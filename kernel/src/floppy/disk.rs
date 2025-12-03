use super::{
    DRIVE_ONE, FloppyError, FloppyPort, ST0_ERR_OR_RESET, TIMEOUT,
    fifo::{self, FloppyCommand},
};
use crate::{floppy::motor, startup, time};
use thiserror::Error;

/// The magnetic encoding mode bit which can be ORed into commands (required for read / write)
pub static MFM_BIT: u8 = 0x40;

/// The number of retries before a disk operation fails and the floppy driver is disabled.
static DISK_RETRIES: u8 = 8;

/// The max number of heads on a floppy disk.
pub static HEADS: u16 = 2;

/// The max number of cylinders on a floppy that sunflower supports.
pub static CYLINDERS: u16 = 80;

/// The max number of sectors per cylinder that sunflower supports.
pub static SECTORS: u16 = 18;

/// The size of a sector which sunflower supports, measured in bytes.
pub static SECTOR_SIZE: usize = 512;

/// Bytes per sector, used in the formula 128^2^X = 512, where X=2.
static BYTES_PER_SECTOR: u8 = 2;

/// An error which occurred due to a disk operation.
#[derive(Error, Debug)]
#[repr(u8)]
pub enum DiskError {
    /// The caller sent a buffer with a length that wasn't a multiple of 512.
    #[error("sent a buf which's len isn't a multiple of 512 ({0})")]
    BadBufLen(u64),

    /// The floppy controller hasn't been or failed to initialise.
    #[error("floppy controller not initialised")]
    ControllerUninit,

    /// Sending read/write command timed out before any errors occurred.
    /// Indicates a major bug in either the time or floppy driver.
    #[error("hit a software timeout while sending command")]
    SendCommandTimeout,

    /// Sending read/write command timed out before any errors occurred.
    /// Indicates a major bug in either the time or floppy driver
    #[error("hit a software timeout while reading command status")]
    ReadStatusTimeout,

    /// Sending read/write command timed out before any errors occurred.
    /// Indicates a major bug in either the time or floppy driver
    #[error("hit a software timeout while transferring data")]
    IoTimeout,

    /// The RQM bit was clear for too long after sending the read/write command.
    #[error("fifo was blocked for too long after sending read/write")]
    FifoTimeout,

    /// The sector or head values aren't valid CHS
    #[error("Found strange floppy sector / head values: {0} / {1}, aborting command...")]
    BadSectOrHead(u8, u8),

    /// Tried writing to read only data.
    #[error("wrote to read-only data")]
    NotWritable,

    /// Attempted reading or writing past the end of the drive.
    #[error("reached the end of the drive")]
    EndOfDrive,

    /// The error bits in st0 were set
    #[error("error bits in st0 set")]
    BadSt0Bits,

    /// The drive isn't ready to be used yet
    #[error("floppy IO failed, drive ins't ready yet")]
    DriveNotReady,

    /// No data was found
    #[error("no data found")]
    NoDataFound,

    /// The controller hit an internal timeout
    #[error("floppy controller timed out")]
    ControllerTimeout,

    /// Reached the end of the cylinder
    #[error("reached end of cylinder")]
    EndOfCylinder,

    /// Some error related to CRC occurred, but it won't tell us what.
    #[error("a CRC error occurred")]
    CRCError,

    /// Hit a cylinder which couldn't be used
    #[error("hit a bad cylinder")]
    BadCylinder,

    /// Somehow tried accessing a different cylinder
    #[error("hit the wrong cylinder")]
    DifferingCylinder,

    /// Hit a sector with a deleted address mark
    #[error("hit a deleted address mark")]
    NoAddressMark,
}

/// Returns the cylinder & sector values from the linear block address.
/// [`Formulas`](https://wiki.osdev.org/Floppy_Disk_Controller#CHS)
fn lba_to_chs(lba: u16) -> (u8, u8, u8) {
    let head = (lba % (HEADS * SECTORS)) / SECTORS;
    let cyl = lba / (SECTORS * HEADS);
    let sector = (lba % (SECTORS * HEADS)) % SECTORS + 1;
    (head as u8, cyl as u8, sector as u8)
}

/// Waits for the RQM bit in the MSR to be set
fn wait_for_rqm() -> Result<(), FloppyError> {
    let start_time = time::get_time();
    while start_time + TIMEOUT > time::get_time() {
        let msr = FloppyPort::msr()?;
        if msr == msr | 0b10000000 {
            return Ok(());
        }
    }
    Err(DiskError::FifoTimeout.into())
}

/// Either sends the read or write command to the controller.
/// # Safety
/// The controller must be not in the middle of another disk operation.
///
/// [`Reference - Section 8.4 Read/Write Data Operations`](http://www.osdever.net/documents/82077AA_FloppyControllerDatasheet.pdf)
#[allow(unused_variables)]
unsafe fn send_read_write(read: bool, ptr: u16, sects: u16) -> Result<(), FloppyError> {
    /// How many retries until we assume that there's either a seek/recalibrate or hardware error.
    static SEEK_RETRIES: u8 = 5;

    /// How many retries until we assume that there's either a read/write data or hardware error.
    static COMMAND_RETRIES: u8 = 8;

    if !startup::FLOPPY_INIT.load() {
        return Err(DiskError::ControllerUninit.into());
    }

    let cmd = if read {
        FloppyCommand::ReadDataWithFlags
    } else {
        FloppyCommand::WriteDataWithFlags
    };

    // Used to tell the controller where to read from
    let (head, cyl, sect) = lba_to_chs(ptr);
    let (end_head, end_cyl, end_sect) = lba_to_chs(ptr + sects - 1);
    if end_cyl >= CYLINDERS as u8 {
        return Err(DiskError::EndOfDrive.into());
    }
    if sect > SECTORS as u8 || sect == 0 || head != end_head || cyl != end_cyl {
        return Err(DiskError::BadSectOrHead(sect, head).into());
    }

    for _ in 0..SEEK_RETRIES {
        // Seek to the cylinder which the read/write command will use
        // Safety: The controller is initialised by this point
        unsafe {
            super::seek(None)?; // fixme: first cmd sent always fails, maybs just send dud command?
            super::seek(Some(cyl))?
        };

        // Attempt to send the command a few times
        let cmd_byte = cmd.clone() as u8;
        for _ in 0..COMMAND_RETRIES {
            wait_for_rqm()?;

            // Safety: Using a valid data range thanks to the above checks
            if unsafe {
                fifo::send_command(
                    &cmd,
                    &[
                        DRIVE_ONE.load() as u8 | (head << 2),
                        cyl,  // start cyl
                        head, // start head
                        sect, // start sector
                        BYTES_PER_SECTOR,
                        end_sect, // end sector
                        0x1B,     // must be 0x1b
                        0xFF,     // must be 0xff
                    ],
                )
                .is_ok()
            } {
                return Ok(());
            }
        }
    }

    Err(DiskError::SendCommandTimeout.into())
}

/// Check if the read or write command passed.
/// # Safety
/// Must be sent right after a read or write command.
unsafe fn read_write_status() -> Result<(), FloppyError> {
    // Loop waiting for a response from the controller
    let start_time = time::get_time();
    let mut err = DiskError::ReadStatusTimeout.into();
    while start_time + TIMEOUT > time::get_time() {
        motor::enable_motor()?;

        // Wait until the RQM bit in the MSR is set
        let msr = FloppyPort::msr()?;
        if msr != msr | 0b10000000 {
            continue;
        }

        // Safety: The check above ensures that we're reading the result bytes from the command
        let (st0, st1, st2, _, _, _, _) = unsafe {
            (
                fifo::read_byte()?,
                fifo::read_byte()?,
                fifo::read_byte()?,
                fifo::read_byte()?,
                fifo::read_byte()?,
                fifo::read_byte()?,
                fifo::read_byte()?,
            )
        };

        // Immediately fail if the data isn't writable
        if st1 | 0x2 == st1 {
            return Err(DiskError::NotWritable.into());
        }

        // Handle the result bits
        if st0 | ST0_ERR_OR_RESET == st0 {
            err = DiskError::BadSt0Bits.into()
        } else if st0 | 0x8 == st0 {
            err = DiskError::DriveNotReady.into()
        } else if st1 | 0x4 == st1 {
            err = DiskError::NoDataFound.into()
        } else if st1 | 0x10 == st1 {
            err = DiskError::ControllerTimeout.into()
        } else if st1 | 0x80 == st1 {
            err = DiskError::EndOfCylinder.into()
        } else if st1 | 0x20 == st1 {
            err = DiskError::CRCError.into()
        } else if st2 | 0x2 == st2 {
            err = DiskError::BadCylinder.into()
        } else if st2 | 0x10 == st2 {
            err = DiskError::DifferingCylinder.into()
        } else if st2 | 0x20 == st2 {
            err = DiskError::CRCError.into()
        } else if st1 | 0x40 == st1 {
            err = DiskError::NoAddressMark.into()
        } else {
            return Ok(());
        }
    }
    Err(err)
}

/// Reads from the floppy drive starting at sector `ptr` into `buf`.
///
/// Fails if the length of `buf` isn't a multiple of 512.
pub fn read(ptr: u16, buf: &mut [u8]) -> Result<(), FloppyError> {
    if buf.is_empty() {
        warn!("useless call to disk::read with an empty buffer");
        return Ok(());
    }

    if !startup::FLOPPY_INIT.load() {
        return Err(DiskError::ControllerUninit.into());
    }

    if !buf.len().is_multiple_of(SECTOR_SIZE) {
        return Err(DiskError::BadBufLen(buf.len() as u64).into());
    }

    // Loop attempting to read the data for a while
    let sects = buf.len() / SECTOR_SIZE;
    let mut err = DiskError::IoTimeout.into();
    'read: for _ in 0..DISK_RETRIES {
        dbg_info!(
            "reading {sects} sectors ({}b) at sect {ptr} from floppy",
            buf.len()
        );

        // Safety: The read and write commands are never ran at the same time
        if let Err(e) = unsafe { send_read_write(true, ptr, sects as u16) } {
            dbg_info!("failed sending floppy read command: {e}. retrying...");
            err = e;
            continue;
        }

        // Fill up the buf with it's new data.
        for byte in buf.iter_mut() {
            wait_for_rqm()?;

            // Safety: The read_write call ensures that we're reading bytes off the drive
            match unsafe { fifo::read_byte() } {
                Ok(data) => *byte = data,
                Err(e) => {
                    warn!("failed floppy read, {e}, retrying up to {DISK_RETRIES} times...");
                    err = e;
                    continue 'read;
                }
            }
        }

        // Safety: Just finished a read command
        unsafe { read_write_status()? };
        motor::disable_motor();
        return Ok(());
    }

    // Safety: Bailing halfway through a read command may leave the controller in an unsynced state
    // and since it can't be reset while a disk operation is in progress, there's no real way to recover
    unsafe { startup::FLOPPY_INIT.store(false) };
    motor::disable_motor();
    println!("Reading from the floppy driver caused an unrecoverable error, {err}");
    println!(fg = LightRed, "All following floppy operations will fail");
    Err(err)
}

/// Writes `buf` into the sector at offset `ptr`.
///
/// Fails if the length of `buf` isn't a multiple of 512.
pub fn write(ptr: u16, buf: &[u8]) -> Result<(), FloppyError> {
    if buf.is_empty() {
        warn!("useless call to disk::write with an empty buffer");
        return Ok(());
    }

    if !startup::FLOPPY_INIT.load() {
        return Err(DiskError::ControllerUninit.into());
    }

    if !buf.len().is_multiple_of(SECTOR_SIZE) {
        return Err(DiskError::BadBufLen(buf.len() as u64).into());
    }

    // Loop attempting to write the data for a while
    let sects = buf.len() / SECTOR_SIZE;
    let mut err = DiskError::IoTimeout.into();
    'write: for _ in 0..DISK_RETRIES {
        dbg_info!(
            "writing {sects} sectors ({}b) at sect {ptr} to floppy",
            buf.len()
        );

        // Safety: The read and write commands are never ran at the same time
        if let Err(e) = unsafe { send_read_write(false, ptr, sects as u16) } {
            dbg_info!("failed sending floppy write command: {e}. retrying...");
            err = e;
            continue;
        }

        // Write the data from the buf.
        for byte in buf {
            wait_for_rqm()?;

            // Safety: The read_write call ensures that we're writing bytes to the drive
            if let Err(e) = unsafe { fifo::send_byte(*byte) } {
                warn!("failed floppy write, {e}, retrying up to {DISK_RETRIES} times...");
                err = e;
                continue 'write;
            }
        }

        /*/ fixme: reading status fails yet sending write succeeds
        #[allow(unused_variables)]
        // Safety: Just finished a write command
        if let Err(e) = unsafe { read_write_status() } {
            warn!("failed retrieving floppy write status: {e}");
        }*/
        motor::disable_motor();
        return Ok(());
    }

    // Safety: Bailing halfway through a write command may leave the controller in an unsynced state
    // and since it can't be reset while a disk operation is in progress, there's no real way to recover
    unsafe { startup::FLOPPY_INIT.store(false) };
    motor::disable_motor();
    println!("Writing to the floppy driver caused an unrecoverable error, {err}");
    println!(fg = LightRed, "All following floppy operations will fail");
    Err(err)
}
