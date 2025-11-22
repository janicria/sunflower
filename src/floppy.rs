use crate::{
    ports, startup, time,
    wrappers::{InitError, InitLater, UnsafeFlag},
};
use core::fmt::Display;
use disk::DiskError;
use fifo::{FifoError, FloppyCommand, SendCommandError, SenseIntState, SenseInterruptError};
use thiserror::Error;

/// Handles data transfers to and from the floppy's disk.
pub mod disk;

/// Handles accessing the FIFO port.
mod fifo;

/// Controls the floppy's motor.
pub mod motor;

/// The base address of the floppy being used by sunflower.
pub static BASE_OFFSET: InitLater<u16> = InitLater::uninit();

/// The disk space of the floppy, measured in KB.
pub static FLOPPY_SPACE: InitLater<u16> = InitLater::uninit();

/// If set drive1 is being used, if not drive 0 is being used.
/// # Flag
/// Falsely toggling this flag causes floppy services to possibly use an invalid drive.
pub static DRIVE_ONE: UnsafeFlag = UnsafeFlag::new(false);

/// Timeout until we assume a command failed, in kernel ticks.
static TIMEOUT: u64 = 30;

/// The number of retries before we assume the controller is unusable.
static RETRIES: u8 = 5;

/// Set after a reset or error occurs.
static ST0_ERR_OR_RESET: u8 = 0xC0;

/// Ports used for floppy operations.
/// To access a port use: `FLOPPY_BASE + Port`
enum FloppyPort {
    /// The digital output register, read & write.
    /// [`Reference`](https://wiki.osdev.org/Floppy_Disk_Controller#DOR_bitflag_definitions)
    DigitalOutputRegister = 2,

    /// The main status register, read only
    /// [`Reference`](https://wiki.osdev.org/Floppy_Disk_Controller#MSR_bitflag_definitions)
    MainStatusRegister = 4,

    /// Data First-in First-Out, read & write
    Fifo = 5,

    /// The config control register, write only
    /// [`CfgCtrl`](https://wiki.osdev.org/Floppy_Disk_Controller#CCR_and_DSR)
    ConfigCtrlRegister = 7,
}

/// The main error type used by the floppy driver.
#[derive(Error, Debug)]
pub enum FloppyError {
    /// An `InitLater` static was unable to be accessed.
    #[error(transparent)]
    InitStatic(#[from] InitError<u16>),

    /// Something went wrong with sending a command.
    #[error(transparent)]
    SendCommand(#[from] SendCommandError),

    /// Something went wrong with sending the sense interrupt command.
    #[error(transparent)]
    SenseInterrupt(#[from] SenseInterruptError),

    /// A disk read or write error occurred.
    #[error("IO error, {0}")]
    ReadOrWrite(#[from] DiskError),

    /// The FIFO port was unable to be accessed in a reasonable amount of time.
    #[error(transparent)]
    FifoTimeout(FifoError),

    /// A specific error occurred in (re)initialisation.
    #[error("{0}")]
    Init(&'static str),
}

/// Size and space information about a floppy drive.
struct FloppyInfo {
    /// The amount of space on the drive, in KB
    space: u16,

    /// The physical size of the floppy.
    /// true  - 5.25 inch / 13.335 cm
    /// false - 3.50 inch / 8.89 cm
    five_inch: bool,
}

impl FloppyPort {
    /// Tries to convert `self` to an I/O port based off the `BASE_OFFSET` static.
    fn add_offset(self) -> Result<u16, InitError<u16>> {
        Ok(self as u16 + *BASE_OFFSET.read()?)
    }

    /// Returns the current valid contained inside the MSR.
    fn msr() -> Result<u8, InitError<u16>> {
        // Safety: just reading from a register
        unsafe { Ok(ports::readb(FloppyPort::MainStatusRegister.add_offset()?)) }
    }
}

impl FloppyInfo {
    /// Tries to create a new value from the type bits.
    /// [`Reference`](https://wiki.osdev.org/CMOS#Register_0x10)
    fn new(bits: u8) -> Option<Self> {
        match bits {
            0 => None, // No drive present
            1 => Some(FloppyInfo {
                space: 360,
                five_inch: true,
            }),
            2 => Some(FloppyInfo {
                space: 1200,
                five_inch: true,
            }),
            3 => Some(FloppyInfo {
                space: 720,
                five_inch: false,
            }),
            4 => Some(FloppyInfo {
                space: 1440,
                five_inch: false,
            }),
            5 => Some(FloppyInfo {
                space: 2880,
                five_inch: false,
            }),
            _ => {
                warn!("unknown floppy type bits ({bits})!");
                None
            }
        }
    }
}

impl Display for FloppyInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let size = if self.five_inch { "5.25" } else { "3.5" };
        write!(f, "{} KB, {size} inches", self.space)
    }
}

/// Initialises the first floppy controller found.
pub fn init() -> Result<(), FloppyError> {
    /// The CMOS register responsible for storing floppy information.
    static CMOS_FLOPPY_REG: u8 = 0x10;

    /// The base offset of the first floppy controller.
    static MAIN_BASE: u16 = 0x3F0;

    /// The base offset of the second floppy controller.
    static SECONDARY_BASE: u16 = 0x370;

    /// The returned value of the Version command from a 82077AA controller.
    static GOOD_VERSION: u8 = 0x90;

    // Safety: Just reading from a register
    let info = unsafe { time::read_cmos_reg(CMOS_FLOPPY_REG) };

    // Also known as master / slave
    let main = FloppyInfo::new(info >> 4);
    let secondary = FloppyInfo::new(info & 0b1111);

    // Figure out which base to use
    if let Some(floppy) = main {
        dbg_info!("Using main floppy - {floppy} with base 0x{MAIN_BASE:X}");
        FLOPPY_SPACE.init(floppy.space)?;
        BASE_OFFSET.init(MAIN_BASE)?;
    } else if let Some(floppy) = secondary {
        dbg_info!("Using secondary floppy - {floppy} with base 0x{SECONDARY_BASE:X}");
        FLOPPY_SPACE.init(floppy.space)?;
        BASE_OFFSET.init(SECONDARY_BASE)?;
        // Safety: Check above ensure that drive 1 is being used
        unsafe { DRIVE_ONE.store(true) };
    } else {
        return Err(FloppyError::Init("No floppy drives found!"));
    }

    motor::enable_motor()?;

    // Check that we have a 82077AA FDC
    // Safety: Version can be sent before initialisation, doesn't take any params & has one result byte
    unsafe {
        fifo::send_command(&FloppyCommand::Version, &[])?;
        if fifo::read_byte()? != GOOD_VERSION {
            return Err(FloppyError::Init("Unsupported controller version!"));
        }
    }

    send_configure()?;

    // Safety: All disk operations fail before FLOPPY_INIT is set
    unsafe {
        init_fdc()?;
        seek(None)?
    };

    // Safety: The controller is well initialised by this point
    unsafe { startup::FLOPPY_INIT.store(true) };
    motor::disable_motor(); // in case it was accidentally left running
    Ok(())
}

/// Sends a formatted configure command to the controller.
/// [`Reference - Section 5.2.7 Configure`](http://www.osdever.net/documents/82077AA_FloppyControllerDatasheet.pdf)
fn send_configure() -> Result<(), FloppyError> {
    /// Implied seek disabled, FIFO enabled, drive polling disabled, threshold = 8
    static COMMAND: u8 = (1 << 6) | (0 << 5) | (1 << 4) | 7;

    // Safety: Sending a well formatted configure command, see above static
    unsafe { fifo::send_command(&FloppyCommand::Configure, &[0, COMMAND, 0])? }

    Ok(())
}

/// (Re)initialises the floppy controller, which can be used to recover it after an error.
/// # Safety
/// Calling this function while disk operations are in progress may corrupt the data on the disk and CRC.
///
/// [`Reference - Section 8.2 Initialization`](http://www.osdever.net/documents/82077AA_FloppyControllerDatasheet.pdf)
unsafe fn init_fdc() -> Result<(), FloppyError> {
    /// Value to set the CCR to enable a 1000 Kbps datarate. Use on 2.88 Mb floppies.
    static CCR_1000_KBPS: u8 = 3;

    /// Value to set the CCR to enable a 500 Kbps datarate. Use on 1.44 or 1.2 Mb floppies.
    static CCR_500_KBPS: u8 = 0;

    motor::enable_motor()?;
    let dor = FloppyPort::DigitalOutputRegister.add_offset()?;

    // Clear the RESET bit, wait for reset to finish, then write the original val back
    // Safety: Just overwriting the DOR for a little bit to reset it, then restoring it
    unsafe {
        let prev = ports::readb(dor);
        ports::writeb(dor, 0);
        time::wait(1);
        ports::writeb(dor, prev);
    }

    // Safety: 4 sense interrupts are required after a reset
    unsafe {
        fifo::sense_interrupt(SenseIntState::FirstReset)?;
        fifo::sense_interrupt(SenseIntState::OtherReset)?;
        fifo::sense_interrupt(SenseIntState::OtherReset)?;
        fifo::sense_interrupt(SenseIntState::OtherReset)?;
    }

    // Update the wiped configuration
    send_configure()?;

    // Get the correct transfer rate based on the floppy's disk size
    let (trans_val, trans_speed) = match FLOPPY_SPACE.read()? {
        1200 | 1440 => (CCR_500_KBPS, 500_000u64),
        2880 => (CCR_1000_KBPS, 1_000_000),
        _ => return Err(FloppyError::Init("Unsupported floppy storage capacity!")),
    };

    // Safety: The check above ensures that we're sending the right transfer speed
    unsafe { ports::writeb(FloppyPort::ConfigCtrlRegister.add_offset()?, trans_val) }

    // Step rate time = 16 - (milliseconds * datarate / 500000), using 8 ms
    let srt = (16 - (8 * trans_speed / 500000)) as u8;

    // Head load time = milliseconds * datarate / 1000000, using 10 ms
    let hlt = (10 * trans_speed / 1000000) as u8;

    // Zero sets the head unload time to max possible value
    let hut = 0;

    // Not DMA flag, disables DMA if true
    let ndma = true as u8;

    // Send the specify command
    // Safety: Hopefully sending a formatted specify command based on the above values
    unsafe {
        fifo::send_command(
            &FloppyCommand::Specify,
            &[((srt << 4) | hut), ((hlt << 1) | ndma)],
        )?
    }

    motor::disable_motor();
    Ok(())
}

/// Sends the recalibrate if `cyl` is `None`, otherwise seeks to `cyl`.
/// # Safety
/// The controller must be initialised and not have a disk transfer in progress.
unsafe fn seek(cyl: Option<u8>) -> Result<(), FloppyError> {
    let mut result = Ok(());
    for _ in 0..RETRIES {
        let (cmd, params) = match cyl {
            None => (FloppyCommand::Recal, &[DRIVE_ONE.load() as u8] as &[u8]),
            Some(cyl) => (FloppyCommand::Seek, &[DRIVE_ONE.load() as u8, cyl] as &[u8]),
        };

        // Safety: Sending a valid command with formatted params with no disk operations happening
        unsafe { fifo::send_command(&cmd, params)? }

        // Check the command's status via sense interrupt
        // Safety: Sent just after a seek, sense interrupt also waits for RQM
        match unsafe { fifo::sense_interrupt(SenseIntState::SeekOrRecal) } {
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
