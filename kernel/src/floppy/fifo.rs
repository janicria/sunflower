/* ---------------------------------------------------------------------------
    Sunflower kernel - sunflowerkernel.org
    Copyright (C) 2026 janicria

    This program is free software: you can redistribute it and/or modify
    it under the terms of the GNU General Public License as published by
    the Free Software Foundation, either version 3 of the License, or
    (at your option) any later version.

    This program is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
    GNU General Public License for more details.

    You should have received a copy of the GNU General Public License
    along with this program.  If not, see <https://www.gnu.org/licenses/>.
--------------------------------------------------------------------------- */

/*!
    kernel/src/floppy/fifo.rs

    Handles FIFO IO and sending commands to the FDC.
    Contained within the floppy module
*/

use thiserror::Error;

use crate::floppy::{
      DRIVE_ONE, FloppyError, FloppyPort, RETRIES, ST0_ERR_OR_RESET, TIMEOUT,
      motor, reset,
};
use crate::{ports, startup, time};

/// Magnetic encoding mode bit, can be ORed into commands.
/// Required for read / write
const MFM_BIT: u8 = 0x40;

/// Commands which can be sent to a floppy drive.
#[derive(Clone)]
#[repr(u8)]
pub enum FloppyCommand {
      /// Set's drive parameters
      Specify            = 3,

      /// Write data to the disk
      WriteDataWithFlags = 5 | MFM_BIT,

      /// Read data from the disk
      ReadDataWithFlags  = 6 | MFM_BIT,

      /// Recalibrate, seeks to cylinder 0
      Recalibrate        = 7,

      /// Acknowledges floppy IRQ
      SenseInterrupt     = 8,

      /// Seek both heads to the specified cylinder
      Seek               = 15,

      /// Responds 0x90 if it's a 82077AA controller
      Version            = 16,

      /// Sends flags to the floppy controller
      Configure          = 19,
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

/// Sends `byte` to the FIFO port.
/// # Safety
/// Writes to the FIFO port.
#[inline(never)]
pub unsafe fn send_byte(byte: u8) -> Result<(), FloppyError> {
      let start_time: u64 = time::get_time();
      while start_time + TIMEOUT > time::get_time() {
            motor::enable_motor()?;

            // Check if MSR = 10XXXXXXb (RQM set & DIO = write), if so, the byte
            // can be sent
            let msr = FloppyPort::msr()?;
            if (msr >> 6) & 0b000000_11 == 0b10 {
                  // Safety: The check above ensures that it's safe to send any
                  // byte and the caller must ensure that
                  // sending the value is also safe
                  unsafe {
                        ports::writeb(FloppyPort::Fifo.add_offset()?, byte)
                  };
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

            // Check if MSR = 11XXXXXXb (RQM set & DIO = read), if so, the byte
            // can be read
            let msr = FloppyPort::msr()?;
            if (msr >> 6) & 0b000000_11 == 0b11 {
                  // Safety: The check above ensures that it's safe to send any
                  // byte and the caller must ensure that
                  // sending the value is also safe
                  motor::disable_motor();
                  return unsafe {
                        Ok(ports::readb(FloppyPort::Fifo.add_offset()?))
                  };
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

/// Sends command `cmd` to the FIFO port with parameters `params`.
/// May reset the controller.
///
/// # Safety
/// The command as well as it's parameters must be safe to send,
/// and a disk operation must not be in progress.
pub unsafe fn send_command(
      cmd: &FloppyCommand, params: &[u8],
) -> Result<(), SendCmdError> {
      let cmd = cmd.clone() as u8;
      let mut err = Ok(());

      'retry: for _ in 0..RETRIES {
            // Safety: The caller must ensure that the command is safe to send
            if unsafe { send_byte(cmd).is_err() } {
                  err = Err(SendCmdError::BadCommand(cmd));
                  cmd_err(cmd)?;
                  continue 'retry;
            }

            for (idx, param) in params.iter().enumerate() {
                  // Safety: The caller must ensure correct params
                  if unsafe { send_byte(*param).is_err() } {
                        err = Err(SendCmdError::BadParameter { cmd, idx });
                        param_err(cmd, idx, *param)?;
                        continue 'retry;
                  }
            }

            return Ok(());
      }

      return err;

      fn cmd_err(cmd: u8) -> Result<(), SendCmdError> {
            dbg_info!("Sending floppy command byte 0x{cmd:X} failed!");
            if !reset() {
                  return Err(SendCmdError::ResetError(cmd));
            }
            Ok(())
      }

      fn param_err(
            cmd: u8, _idx: usize, _param: u8,
      ) -> Result<(), SendCmdError> {
            dbg_info!(
                  "Sending floppy param 0x{_param:X} ({_idx}) \
            to command 0x{cmd:X} failed!"
            );
            if !reset() {
                  return Err(SendCmdError::ResetError(cmd));
            }
            Ok(())
      }

      fn reset() -> bool {
            dbg_info!("Resetting FDC!");
            // Safety: The caller must ensure that there are no disk operations
            unsafe { reset::init_fdc().is_ok() }
      }
}

/// The error returned from `send_command`.
#[derive(Error, Debug)]
pub enum SendCmdError {
      #[error("sent either a bad command ({0}:X) or one at the wrong time")]
      BadCommand(u8),

      #[error(
            "sent either a bad parameter (idx {idx} in command {cmd:X}) \
            or one at the wrong time"
      )]
      BadParameter { cmd: u8, idx: usize },

      #[error(
            "encountered an error while resetting in after attempting to \
            send command 0x{0:X}"
      )]
      ResetError(u8),
}

/// Sends the recalibrate command if `cyl` is `None`, otherwise seeks to `cyl`.
///
/// # Safety
/// The controller must be initialised and not have a disk transfer in progress.
pub unsafe fn seek(cyl: Option<u8>) -> Result<(), FloppyError> {
      let mut ret = Ok(());

      'retry: for _ in 0..RETRIES {
            if let Some(cyl) = cyl {
                  let params = &[DRIVE_ONE.load() as u8, cyl];
                  // Safety: Caller must ensure valid cyl & no disk operations
                  unsafe { send_command(&FloppyCommand::Seek, params)? }
            } else {
                  let params = &[DRIVE_ONE.load() as u8];
                  // Safety: The caller must ensure no disk operations
                  unsafe { send_command(&FloppyCommand::Recalibrate, params)? }
            }

            if let Err(e) = unsafe { sense_int(SenseIntState::SeekOrRecal) } {
                  // There's no point checking for a ResendCommand error, since
                  // this function already retries after errors, and
                  // receiving a ResendCommand on the final retry would also
                  // indicate an error
                  ret = Err(e);
                  continue 'retry;
            }

            return Ok(());
      }

      ret
}

/// Sends the sense interrupt command and checks if it passed.
///
/// # Safety
/// The caller must ensure that this function is only called **ONCE**,
/// immediately after a Seek or Recalibrate, and **FOUR** TIMES after a Reset
/// command.
/// The correct state must be passed via the `state` enum, and a disk
/// operation must not be in progress if the state is
/// [`SenseIntState::SeekOrRecal`].
pub unsafe fn sense_int(state: SenseIntState) -> Result<(), FloppyError> {
      /// Set after a recalibrate or seek completed successfully.
      const RECALIBRATE_SEEK_PASSED: u8 = 0x20;

      /// If this is the only bit set in st0, the controller has locked up.
      /// Can be recovered from using a reset.
      const CONTROLLER_LOCK_UP: u8 = 0x80;

      // Safety: The caller must ensure that it's safe to send the command
      let (st0, _) = unsafe {
            send_byte(FloppyCommand::SenseInterrupt as u8)?;
            (read_byte()?, read_byte()?) // only care about st0
      };
      let drive_num = DRIVE_ONE.load() as u8;
      let seek_recal_ok = st0 == RECALIBRATE_SEEK_PASSED | drive_num;
      let reset_or_err = st0 == ST0_ERR_OR_RESET | drive_num;

      return if (reset_or_err && state == SenseIntState::FirstReset) ||
            (seek_recal_ok && state == SenseIntState::SeekOrRecal)
      {
            Ok(())
      } else if st0 == CONTROLLER_LOCK_UP {
            controller_lockup(state != SenseIntState::SeekOrRecal)?;
            Err(SenseIntError::ControllerLockup.into())
      } else if !seek_recal_ok && state == SenseIntState::SeekOrRecal {
            // No pass & no error means drive needs another command
            // to move across more cylinders
            Err(SenseIntError::ResendCommand.into())
      } else if reset_or_err && state == SenseIntState::SeekOrRecal {
            Err(SenseIntError::CommandFailed.into())
      } else if state == SenseIntState::OtherReset {
            // Preceding reset sense ints don't have the reset bit set
            Ok(())
      } else {
            Err(SenseIntError::UnknownError.into())
      };

      fn controller_lockup(reset: bool) -> Result<(), FloppyError> {
            if reset {
                  print!("FDC reset caused a controller lockup!");
                  println!(
                        fg = LightRed,
                        "All following floppy operations will fail"
                  );
                  // Safety: If an error occurs during a reset
                  // there's no real way to recover from it, so
                  // it's a good idea to also lockup the driver
                  unsafe { startup::FLOPPY_INIT.store(false) };
            } else {
                  dbg_info!("FDC locked up in a seek or Recalibrate!");
                  // Safety: Caller ensures a disk operation  isn't happening
                  unsafe { reset::init_fdc()? }
            }
            Ok(())
      }
}

/// The error returned from `sense_interrupt`.
#[derive(Error, Debug, PartialEq)]
pub enum SenseIntError {
      #[error("please resend the seek or recalibrate command")]
      ResendCommand,

      #[error(
            "controller lockup detected due to sending a sense interrupt \
        command at the wrong time!"
      )]
      ControllerLockup,

      #[error("either a seek or recalibrate failed sending")]
      CommandFailed,

      #[error("a reset, seek or recalibrate command failed to send")]
      UnknownError,
}
