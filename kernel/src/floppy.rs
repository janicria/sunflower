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
    kernel/src/floppy.rs

    The floppy module handles the FDC and floppy disk IO.
    This file is responsible for initialising the FDC and filesystem.

    Contains 6 submodules:
    * disk.rs - Handles floppy disk reading and writing
    * fifo.rs - Handles FIFO IO and sending commands to the FDC
    * floppyfs.rs - Initialises the "filesystem" - will be removed soon when snugfs is done
    * motor.rs - Allows enabling and disabling floppy motors
    * reset.rs - Handles sending reset commands to FDC
*/

use crate::{
    exit_on_err,
    floppy::fifo::FloppyCommand,
    ports,
    startup::{self, ExitCode},
    time,
};
use core::fmt::Display;
use disk::DiskError;
use fifo::{FifoIOError, SendCommandError, SenseInterruptError};
use libutil::{InitError, InitLater, UnsafeFlag};
use thiserror::Error;

pub mod disk;
mod fifo;
pub mod floppyfs;
pub mod motor;
mod reset;

/// The base address of the floppy being used by sunflower.
pub static BASE_OFFSET: InitLater<u16> = InitLater::uninit();

/// The disk space of the floppy, measured in KB.
pub static FLOPPY_SPACE: InitLater<u16> = InitLater::uninit();

/// If set drive1 is being used, if not drive 0 is being used.
/// # Flag
/// Falsely toggling this flag causes floppy services to possibly use an invalid drive.
pub static DRIVE_ONE: UnsafeFlag = UnsafeFlag::new(false);

/// Timeout until we assume a command failed, in kernel ticks.
const TIMEOUT: u64 = 30;

/// The number of retries before we assume the controller is unusable.
const RETRIES: u8 = 5;

/// Set after a reset or error occurs.
const ST0_ERR_OR_RESET: u8 = 0xC0;

/// The max number of heads on a floppy disk.
const HEADS: u16 = 2;

/// The max number of cylinders on a floppy that sunflower supports.
const CYLINDERS: u16 = 80;

/// The max number of sectors per cylinder that sunflower supports.
const SECTORS: u16 = 18;

/// The size of a sector which sunflower supports, measured in bytes.
const SECTOR_SIZE: usize = 512;

/// The length of each cylinder, in bytes.
const CYL_BOUNDARY: usize = SECTORS as usize * SECTOR_SIZE;

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
    FifoTimeout(FifoIOError),

    /// Some other error occurred.
    #[error("{0}")]
    Other(&'static str),
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

/// Runs the init function for the FDC.
pub fn init_wrapper() -> ExitCode<FloppyError> {
    exit_on_err!(init());
    ExitCode::Ok
}

/// Initialises the first floppy controller found.
fn init() -> Result<(), FloppyError> {
    /// The CMOS register responsible for storing floppy information.
    const CMOS_FLOPPY_REG: u8 = 0x10;

    /// The base offset of the first floppy controller.
    const MAIN_BASE: u16 = 0x3F0;

    /// The base offset of the second floppy controller.
    const SECONDARY_BASE: u16 = 0x370;

    /// The returned value of the Version command from a 82077AA controller.
    const GOOD_VERSION: u8 = 0x90;

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
        return Err(FloppyError::Other("No floppy drives found!"));
    }

    motor::enable_motor()?;

    // Check that we have a 82077AA FDC
    // Safety: Version can be sent before initialisation, doesn't take any params & has one result byte
    unsafe {
        fifo::send_command(&FloppyCommand::Version, &[])?;
        if fifo::read_byte()? != GOOD_VERSION {
            return Err(FloppyError::Other("Unsupported controller version!"));
        }
    }

    reset::send_configure()?;

    // Safety: All disk operations fail before FLOPPY_INIT is set, so we know they're not going
    unsafe {
        reset::init_fdc()?;
        fifo::seek(None)?
    };

    // Safety: The controller is well initialised by this point
    unsafe { startup::FLOPPY_INIT.store(true) };
    motor::disable_motor(); // in case it was accidentally left running
    Ok(())
}
