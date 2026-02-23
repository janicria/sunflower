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
    kernel/src/floppy/disk.rs

    Handles floppy disk reading and writing.
    Contained within the floppy module
*/

use core::sync::atomic::{AtomicU64, Ordering};

use thiserror::Error;

use crate::floppy::{
      CYLINDERS, DRIVE_ONE, FloppyCommand, FloppyError, FloppyPort, HEADS,
      SECTOR_SIZE, SECTORS, ST0_ERR_OR_RESET, TIMEOUT, fifo, motor,
};
use crate::{startup, time};

/// The number of successfully read bytes from the floppy drive.
pub static READ_BYTES: AtomicU64 = AtomicU64::new(0);

/// The number of successfully written bytes to the floppy drive.
pub static WRITTEN_BYTES: AtomicU64 = AtomicU64::new(0);

/// The number of retries before a read or write fails
/// and the floppy driver is disabled.
const DISK_RETRIES: u8 = 8;

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

/// Reads from the floppy drive starting at sector `ptr` into `buf`.
///
/// Fails if the length of `buf` isn't a multiple of 512.
pub fn read(ptr: u64, buf: &mut [u8]) -> Result<(), FloppyError> {
      if buf.is_empty() {
            warn!("floppy: useless read with an empty buffer");
            return Ok(());
      }

      if !startup::FLOPPY_INIT.load() {
            return Err(DiskError::ControllerUninit.into());
      }

      if !buf.len().is_multiple_of(SECTOR_SIZE) {
            return Err(DiskError::BadBufLen(buf.len() as u64).into());
      }

      let sects = (buf.len() / SECTOR_SIZE) as u16;
      let ptr = ptr as u16;
      let mut err = DiskError::IoTimeout.into();

      dbg_info!(
            "floppy: reading sectors {ptr}-{} ({}b)",
            ptr + sects,
            buf.len()
      );

      'retry: for _ in 0..DISK_RETRIES {
            if let Err(e) = send_read_write(true, ptr, sects) {
                  dbg_info!(
                        "floppy: failed sending read command: {e}. retrying..."
                  );
                  err = e;
                  continue 'retry;
            }

            for byte in buf.iter_mut() {
                  wait_for_rqm()?;

                  // Safety: The read_write call ensures that we're
                  // reading bytes off the drive
                  match unsafe { fifo::read_byte() } {
                        Ok(data) => *byte = data,
                        Err(e) => {
                              warn!("floppy: read failed: {e}, retrying...");
                              err = e;
                              continue 'retry;
                        }
                  }
            }

            // Safety: Just finished a read command
            unsafe { read_write_status()? };
            motor::disable_motor();
            READ_BYTES.fetch_add(buf.len() as u64, Ordering::Relaxed);
            return Ok(());
      }

      // Safety: Bailing halfway through a read command may leave the controller
      // in an unsynced state and since it can't be reset while a disk
      // operation is in progress, there's no real way to recover
      unsafe { startup::FLOPPY_INIT.store(false) };
      motor::disable_motor();
      println!(
            "Reading from the floppy driver caused an \
            unrecoverable error, {err}"
      );
      println!(fg = LightRed, "All following floppy operations will fail");
      Err(err)
}

/// Writes `buf` into the sector at offset `ptr`.
///
/// Fails if the length of `buf` isn't a multiple of 512.
pub fn write(ptr: u64, buf: &[u8]) -> Result<(), FloppyError> {
      if buf.is_empty() {
            warn!("floppy: useless write with an empty buffer");
            return Ok(());
      }

      if !startup::FLOPPY_INIT.load() {
            return Err(DiskError::ControllerUninit.into());
      }

      if !buf.len().is_multiple_of(SECTOR_SIZE) {
            return Err(DiskError::BadBufLen(buf.len() as u64).into());
      }

      // Loop attempting to write the data for a while
      let sects = (buf.len() / SECTOR_SIZE) as u16;
      let ptr = ptr as u16;
      let mut err = DiskError::IoTimeout.into();

      dbg_info!(
            "floppy: writing sectors {ptr}-{} ({}b)",
            ptr + sects,
            buf.len()
      );

      'retry: for _ in 0..DISK_RETRIES {
            if let Err(e) = send_read_write(false, ptr, sects) {
                  dbg_info!(
                        "floppy: failed sending write \
                        command: {e}. retrying..."
                  );
                  err = e;
                  continue 'retry;
            }

            // Write the data from the buf.
            for byte in buf.iter() {
                  wait_for_rqm()?;

                  // Safety: The read_write call ensures that we're writing
                  // bytes to the drive
                  if let Err(e) = unsafe { fifo::send_byte(*byte) } {
                        warn!("floppy: write failed: {e}, retrying...");
                        err = e;
                        continue 'retry;
                  }
            }

            /*/ FIXME: reading status fails and the next command being sent
            fails, causing a fdc reset
            #[allow(unused_variables)]
            // Safety: Just finished a write command
            if let Err(e) = unsafe { read_write_status() } {
                warn!("failed retrieving floppy write status: {e}");
            }*/
            motor::disable_motor();
            WRITTEN_BYTES.fetch_add(buf.len() as u64, Ordering::Relaxed);
            return Ok(());
      }

      // Safety: Bailing halfway through a write command may leave the
      // controller in an unsynced state and since it can't be reset while
      // a disk operation is in progress, there's no real way to recover
      unsafe { startup::FLOPPY_INIT.store(false) };
      motor::disable_motor();
      println!(
            "Writing to the floppy driver caused an unrecoverable error, {err}"
      );
      println!(fg = LightRed, "All following floppy operations will fail");
      Err(err)
}

/// Sends either the read or write command to the controller.
///
/// See section 8.4 Read/Write Data Operations of the datasheet.
fn send_read_write(
      read: bool, ptr: u16, sects: u16,
) -> Result<(), FloppyError> {
      /// How many retries until we assume that there's either a
      /// seek/recalibrate or hardware error.
      const SEEK_RETRIES: u8 = 5;

      /// Bytes per sector, used in the formula 128^2^X = 512, where X=2.
      const BYTES_PER_SECTOR: u8 = 2;

      if !startup::FLOPPY_INIT.load() {
            return Err(DiskError::ControllerUninit.into());
      }

      let (start_head, start_cyl, start_sect) = lba_to_chs(ptr);
      let (end_head, end_cyl, end_sect) = lba_to_chs(ptr + sects - 1);
      if end_cyl >= CYLINDERS as u8 {
            return Err(DiskError::EndOfDrive.into());
      }
      if start_sect > SECTORS as u8 ||
            start_sect == 0 ||
            start_head != end_head ||// cannot IO over cyl boundaries
            start_cyl != end_cyl
      {
            return Err(DiskError::BadSectOrHead(start_sect, start_head).into());
      }

      let cmd = if read {
            FloppyCommand::ReadDataWithFlags
      } else {
            FloppyCommand::WriteDataWithFlags
      };
      let params = &[
            DRIVE_ONE.load() as u8 | (start_head << 2),
            start_cyl,
            start_head,
            start_sect,
            BYTES_PER_SECTOR,
            end_sect,
            0x1B,
            0xFF,
      ];

      for _ in 0..SEEK_RETRIES {
            // Seek to the cylinder which the read/write command will use
            // Safety: The controller is initialised by this point
            unsafe {
                  // FIXME: first cmd sent always fails after a write,
                  // probs due to broken write read_write_status check
                  fifo::seek(None)?;
                  fifo::seek(Some(start_cyl))?
            };

            wait_for_rqm()?;

            // Safety: Only one disk command is ever ran at a time,
            // meaning one can never be in progress here
            if unsafe { fifo::send_command(&cmd, params).is_ok() } {
                  return Ok(());
            }
      }

      return Err(DiskError::SendCommandTimeout.into());

      fn lba_to_chs(lba: u16) -> (u8, u8, u8) {
            let head = (lba % (HEADS * SECTORS)) / SECTORS;
            let cyl = lba / (SECTORS * HEADS);
            let sector = (lba % (SECTORS * HEADS)) % SECTORS + 1;
            (head as u8, cyl as u8, sector as u8)
      }
}

/// Check if the read or write command passed.
/// # Safety
/// Must be sent right after a read or write command.
unsafe fn read_write_status() -> Result<(), FloppyError> {
      wait_for_rqm()?;

      // Safety: The check above ensures that we're reading the result
      // bytes from the command
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
            Err(DiskError::BadSt0Bits.into())
      } else if st0 | 0x8 == st0 {
            Err(DiskError::DriveNotReady.into())
      } else if st1 | 0x4 == st1 {
            Err(DiskError::NoDataFound.into())
      } else if st1 | 0x10 == st1 {
            Err(DiskError::ControllerTimeout.into())
      } else if st1 | 0x80 == st1 {
            Err(DiskError::EndOfCylinder.into())
      } else if st1 | 0x20 == st1 {
            Err(DiskError::CRCError.into())
      } else if st2 | 0x2 == st2 {
            Err(DiskError::BadCylinder.into())
      } else if st2 | 0x10 == st2 {
            Err(DiskError::DifferingCylinder.into())
      } else if st2 | 0x20 == st2 {
            Err(DiskError::CRCError.into())
      } else if st1 | 0x40 == st1 {
            Err(DiskError::NoAddressMark.into())
      } else {
            Ok(())
      }
}

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
      #[error("hit a software timeout while transferring data")]
      IoTimeout,

      /// The RQM bit was clear for too long after sending the read/write
      /// command.
      #[error("fifo was blocked for too long after sending read/write")]
      FifoTimeout,

      /// The sector or head values aren't valid CHS
      #[error(
            "found strange floppy sector / head values: {0} / {1} \
        did you io over a cylinder boundary?"
      )]
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
