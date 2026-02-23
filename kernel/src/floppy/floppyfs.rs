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
    kernel/src/floppy/floppyfs.rs

    Initialises the "filesystem" - will be removed soon.
    Contained within the floppy module
*/

use core::sync::atomic::{AtomicBool, Ordering};

use libfs::header::{FilesystemHeader, FsFeatures};
use libfs::init::{self, ReadTblError};
use libfs::table::{BlockBitmap, InodeTable};
use libfs::{BlockPtr, INODES, INode, MAGIC};
use libutil::ExclusiveMap;
use thiserror::Error;

use crate::floppy::{CYL_BOUNDARY, FloppyError, SECTOR_SIZE, SECTORS, disk};
use crate::startup::{self, ExitCode};
use crate::{exit_on_err, interrupts};

/// Has floppyfs been initialised yet?
pub static FLOPPYFS_INIT: AtomicBool = AtomicBool::new(false);

/// The year value in the floppy fsheader.
const YEAR: u16 = crate::env_as_int!("SFK_FLOPPYFS_YEAR", u16);

/// The day value in the floppy fsheader.
const DAY: u16 = crate::env_as_int!("SFK_FLOPPYFS_DAY", u16);

/// A good default filesystem header.
const GOOD_FS_HEADER: FilesystemHeader = FilesystemHeader::new(
      [
            // "floppy drive"
            102, 108, 111, 112, 112, 121, 32, 100, 114, 105, 118, 101, 0, 0, 0,
            0,
      ],
      DAY,
      YEAR,
      0,
      FsFeatures::FLOPPY,
);

libfs::table_statics!();

/// A wrapper over [`disk::write`], which allows writing over cylinder
/// boundaries.
pub fn write(block: u64, buf: &[u8]) -> Result<(), FloppyError> {
      // Since block can start anywhere relative to a cyl boundary,
      // we have to make sure to use a smaller buf for the first write
      let fst_cyl_distance = SECTORS as usize - block as usize;
      let fst_cyl_boundary = (fst_cyl_distance * SECTOR_SIZE).min(buf.len());
      disk::write(block, &buf[..fst_cyl_boundary])?;

      let block = block + fst_cyl_distance as u64;
      for (idx, buf) in buf[fst_cyl_boundary..].chunks(CYL_BOUNDARY).enumerate()
      {
            let block = block + (idx * CYL_BOUNDARY) as u64;
            disk::write(block, buf)?
      }

      Ok(())
}

/// A wrapper over [`disk::read`], which allows reading over cylinder
/// boundaries.
pub fn read(block: u64, buf: &mut [u8]) -> Result<(), FloppyError> {
      // Since block can start anywhere relative to a cyl boundary,
      // we have to make sure to use a smaller buf for the first read
      let fst_cyl_distance = SECTORS as usize - block as usize;
      let fst_cyl_boundary = (fst_cyl_distance * SECTOR_SIZE).min(buf.len());
      disk::read(block, &mut buf[..fst_cyl_boundary])?;

      let block = block + fst_cyl_distance as u64;
      for (idx, buf) in
            buf[fst_cyl_boundary..].chunks_mut(CYL_BOUNDARY).enumerate()
      {
            let block = block + (idx as u64 * SECTORS as u64);
            disk::read(block, buf)?
      }

      Ok(())
}

/// Initialises and mounts the floppy filesystem.
#[rustfmt::skip]
pub fn init_floppyfs() -> ExitCode<InitError> {
      if !startup::FLOPPY_INIT.load() {
            return ExitCode::Error(InitError::NoFloppyDriver);
      }

      // Read the filesystem's header
      let mut buf = [0; size_of::<FilesystemHeader>()];
      exit_on_err!(disk::read(0, &mut buf));
      let mut fsheader = FilesystemHeader::from_raw(buf);

      // Check that the fs is formatted
      if fsheader.magic != MAGIC {
            dbg_info!("Bad filesystem magic found");
            if !interrupts::kbd_wait_for_response("Format floppy drive", true) {
                  return ExitCode::Error(InitError::CorruptDrive);
            }
            fsheader = GOOD_FS_HEADER;
            exit_on_err!(init::reformat_drive(&GOOD_FS_HEADER, write))
      }

      // Check if the filesystem is a newer version
      let fs_release = fsheader.release();
      if fs_release > GOOD_FS_HEADER.release() {
            dbg_info!(
                  "Filesystem has newer release than kernel, some features may not be supported"
            )
      }

      dbg_info!(
            "Found floppy filesystem: {}, released {fs_release}
Filesystem features: {}",
            str::from_utf8(&fsheader.name)
                  .unwrap_or("filesystem contains bad name"),
            fsheader.features()
      );

      let (_nods, _blocks) =
            exit_on_err!(init::read_table(&INODE_TBL, &BLOCK_BMP, read));
      dbg_info!(
            "Read inode table, active inodes: {_nods}, used blocks: {_blocks}"
      );
      FLOPPYFS_INIT.store(true, Ordering::Relaxed);
      ExitCode::Ok
}

/// An error created when trying to initialise the floppy filesystem.
#[derive(Error, Debug)]
pub enum InitError {
      #[error("The floppy driver failed!")]
      NoFloppyDriver,

      #[error("floppy driver error: {0}")]
      FloppyError(#[from] FloppyError),

      #[error("The floppy drive was corrupt!")]
      CorruptDrive,

      #[error("read table error: {0}")]
      TableError(#[from] ReadTblError<FloppyError>),
}
