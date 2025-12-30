//! Allows raw FIFO IO as well as sending commands to it.

use crate::{
    floppy::{
        DRIVE_ONE, FloppyError, FloppyPort, RETRIES, ST0_ERR_OR_RESET, TIMEOUT, motor, reset,
    },
    ports, startup, time,
};
use thiserror::Error;

/// The magnetic encoding mode bit which can be ORed into commands (required for read / write)
const MFM_BIT: u8 = 0x40;

/// Sends `byte` to the FIFO port.
/// # Safety
/// Writes to the FIFO port.
#[inline(never)]
pub unsafe fn send_byte(byte: u8) -> Result<(), FloppyError> {
    let start_time: u64 = time::get_time();
    while start_time + TIMEOUT > time::get_time() {
        motor::enable_motor()?;

        // Check if MSR = 10XXXXXXb (RQM set & DIO = write), if so, the byte can be sent
        let msr = FloppyPort::msr()?;
        if (msr >> 6) & 0b000000_11 == 0b10 {
            // Safety: The check above ensures that it's safe to send any byte
            // and the caller must ensure that sending the value is also safe
            unsafe { ports::writeb(FloppyPort::Fifo.add_offset()?, byte) };
            motor::disable_motor();
            return Ok(());
        }
    }

    motor::disable_motor();
    Err(FloppyError::FifoTimeout(FifoIOError::Write(byte)))
}

/// Reads a byte from the FIFO port.
/// # Safety
/// Reads from the FIFO port.
#[inline(never)]
pub unsafe fn read_byte() -> Result<u8, FloppyError> {
    let start_time = time::get_time();
    while start_time + TIMEOUT > time::get_time() {
        motor::enable_motor()?;

        // Check if MSR = 11XXXXXXb (RQM set & DIO = read), if so, the byte can be read
        let msr = FloppyPort::msr()?;
        if (msr >> 6) & 0b000000_11 == 0b11 {
            // Safety: The check above ensures that it's safe to send any byte
            // and the caller must ensure that sending the value is also safe
            motor::disable_motor();
            return unsafe { Ok(ports::readb(FloppyPort::Fifo.add_offset()?)) };
        }
    }

    motor::disable_motor();
    Err(FloppyError::FifoTimeout(FifoIOError::Read))
}

/// The error returned from FIFO operations.
#[derive(Error, Debug)]
pub enum FifoIOError {
    #[error("read byte from the FIFO port at the wrong time")]
    Read,

    #[error("wrote byte 0x{0:X} to the FIFO port at the wrong time")]
    Write(u8),
}

/// Commands which can be sent to a floppy drive.
#[derive(Clone)]
#[repr(u8)]
pub enum FloppyCommand {
    /// Set's drive parameters
    Specify = 3,

    /// Write data to the disk
    WriteDataWithFlags = 5 | MFM_BIT,

    /// Read data from the disk
    ReadDataWithFlags = 6 | MFM_BIT,

    /// Recalibrate, seeks to cylinder 0
    Recal = 7,

    /// Acknowledges floppy IRQ
    SenseInterrupt = 8,

    /// Seek both heads to the specified cylinder
    Seek = 15,

    /// Responds 0x90 if it's a 82077AA controller
    Version = 16,

    /// Sends flags to the floppy controller
    Configure = 19,
}

/// Sends command `cmd` to the FIFO port with parameters `params`.
/// Resets the controller if an error occurs.
///
/// # Safety
/// The command as well as it's parameters must be safe to send and a disk operation must not be in progress.
pub unsafe fn send_command(cmd: &FloppyCommand, params: &[u8]) -> Result<(), SendCommandError> {
    let cmd = cmd.clone() as u8;
    let mut res = Ok(());

    fn reinit(cmd: u8, err: SendCommandError) -> SendCommandError {
        // Safety: It's the responsibility of the caller to ensure that there isn't a disk operation happening
        if unsafe { reset::init_fdc().is_err() } {
            SendCommandError::ResetError(cmd)
        } else {
            err
        }
    }

    'command: for _ in 0..RETRIES {
        // Safety: The caller must ensure that the command is safe to send
        if unsafe { send_byte(cmd).is_err() } {
            dbg_info!("Sending floppy command byte 0x{cmd:X} failed!");
            res = Err(reinit(cmd, SendCommandError::BadCommand(cmd)));
            continue;
        }

        // Send the parameter bytes after the command
        for (idx, param) in params.iter().enumerate() {
            // Safety: The caller must ensure that parameters are correct
            if unsafe { send_byte(*param).is_err() } {
                dbg_info!("Sending floppy param 0x{param:X} to command 0x{cmd:X} failed!");
                res = Err(reinit(cmd, SendCommandError::BadParameter { cmd, idx }));
                continue 'command;
            }
        }

        return Ok(());
    }

    res
}

/// The error returned from `send_command`.
#[derive(Error, Debug)]
pub enum SendCommandError {
    #[error("sent either a bad command ({0}:X) or one at the wrong time")]
    BadCommand(u8),

    #[error("sent either a bad parameter (idx {idx} in command {cmd:X}) or one at the wrong time")]
    BadParameter { cmd: u8, idx: usize },

    #[error("encountered an error while resetting in after attempting to send command 0x{0:X}")]
    ResetError(u8),
}

/// Sends the recalibrate command if `cyl` is `None`, otherwise seeks to `cyl`.
/// # Safety
/// The controller must be initialised and not have a disk transfer in progress.
pub unsafe fn seek(cyl: Option<u8>) -> Result<(), FloppyError> {
    let mut result = Ok(());
    for _ in 0..RETRIES {
        let (cmd, params) = match cyl {
            None => (FloppyCommand::Recal, &[DRIVE_ONE.load() as u8] as &[u8]),
            Some(cyl) => (FloppyCommand::Seek, &[DRIVE_ONE.load() as u8, cyl] as &[u8]),
        };

        // Safety: Sending a valid command with formatted params with no disk operations happening
        unsafe { send_command(&cmd, params)? }

        // Check the command's status via sense interrupt
        // Safety: Sent just after a seek, sense interrupt also waits for RQM
        match unsafe { sense_interrupt(SenseIntState::SeekOrRecal) } {
            Ok(()) => return Ok(()),
            Err(e) => {
                if let FloppyError::SenseInterrupt(ref e) = e
                    && matches!(e, SenseInterruptError::ResendCommand)
                {
                } else {
                    result = Err(e)
                }
            }
        }
    }

    result
}

/// Sends the sense interrupt command and checks if it passed.
///
/// # Safety
/// The caller must ensure that this function is only called **ONCE**, immediately after a Seek or Recalibrate,
/// and **FOUR** TIMES after a Reset command. The correct state must be passed via the `state` enum,
/// and a disk operation must not be in progress if the state is [`SenseIntState::SeekOrRecal`].
pub unsafe fn sense_interrupt(state: SenseIntState) -> Result<(), FloppyError> {
    /// Set after a recalibrate or seek completed successfully.
    static RECALIBRATE_SEEK_PASSED: u8 = 0x20;

    /// If this is the only bit set in st0, the controller has locked up and requires a reset.
    static CONTROLLER_LOCK_UP: u8 = 0x80;

    // Safety: The caller must ensure that it's safe to send a sense int command
    let (st0, _) = unsafe {
        send_byte(FloppyCommand::SenseInterrupt as u8)?;
        (read_byte()?, read_byte()?)
    };

    let drive_num = DRIVE_ONE.load() as u8;
    let seek_recal_passed = st0 == RECALIBRATE_SEEK_PASSED | drive_num; // if st0 = 0x20 | drive num, the cmd completed
    let reset_passed = st0 == ST0_ERR_OR_RESET | drive_num; // if st0 = 0xC0 | drive num, the reset completed

    if (reset_passed && state == SenseIntState::FirstReset)
        || (seek_recal_passed && state == SenseIntState::SeekOrRecal)
    {
        // if command X passed and command X was sent, then the sent command passed
        Ok(())
    } else if !seek_recal_passed && state == SenseIntState::SeekOrRecal {
        // if the seek or recal didn't complete, but there wasn't an error, then
        // another command should be sent as the drive needs to move across more cylinders
        Err(SenseInterruptError::ResendCommand.into())
    } else if st0 == CONTROLLER_LOCK_UP {
        // If st0 == 0x80, we sent a sense int at the wrong time and the controller has locked up
        if state == SenseIntState::SeekOrRecal {
            dbg_info!("Controller locked up in a seek or Recalibrate!");
            // Safety: The caller must ensure that a disk operation isn't happening
            unsafe { reset::init_fdc()? };
        } else {
            print!("An unrecoverable error occurred in the floppy driver! ");
            println!(fg = LightRed, "All following floppy operations will fail");
            // Safety: If an error occurs during a reset then there's no real way
            // to fix it, so it's probably a good idea to lock up the FDC driver
            unsafe { startup::FLOPPY_INIT.store(false) };
        }
        Err(SenseInterruptError::ControllerLockup.into())
    } else if reset_passed && state == SenseIntState::SeekOrRecal {
        // the reset passed bits are also used to indicate an error occurred if not from a reset command
        Err(SenseInterruptError::CommandFailed.into())
    } else if state == SenseIntState::OtherReset {
        // Sense interrupts sent which aren't the first after a reset don't have the `ST0_ERR_OR_RESET` bits set
        Ok(())
    } else {
        // if we make it here, the command failed, but for some other reason
        Err(SenseInterruptError::UnknownError.into())
    }
}

/// The reason a sense interrupt is being ran.
#[derive(PartialEq)]
pub enum SenseIntState {
    /// Right after a seek or Recalibrate command.
    SeekOrRecal,

    /// The first sense interrupt after a reset.
    FirstReset,

    /// Any other sense interrupt after a reset.
    OtherReset,
}

/// The error returned from `sense_interrupt`.
#[derive(Error, Debug)]
pub enum SenseInterruptError {
    #[error("please resend the seek or recalibrate command")]
    ResendCommand,

    #[error(
        "controller lockup detected due to sending a sense interrupt command at the wrong time!"
    )]
    ControllerLockup,

    #[error("either a seek or recalibrate failed sending")]
    CommandFailed,

    #[error("a reset, seek or recalibrate command failed to send")]
    UnknownError,
}
