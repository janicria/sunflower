use crate::{
    exit_on_err,
    floppy::{FloppyError, disk},
    interrupts,
    startup::{self, ExitCode},
};
use core::sync::atomic::{AtomicBool, Ordering};
use libfs::{
    FilesystemFeatures, FilesystemHeader, INODES, INode, MAGIC,
    init::{self, ReadTableError},
    table::{self, AllocINodeError, InodeBitmap, InodeIOError, InodeTable},
};
use libutil::ExclusiveMap;
use thiserror::Error;

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
        102, 108, 111, 112, 112, 121, 32, 100, 114, 105, 118, 101, 0, 0, 0, 0,
    ],
    DAY,
    YEAR,
    [0; 64], // mount at root dir
    0,
    FilesystemFeatures::FLOPPY,
);

libfs::inode_statics!();

/// See [`alloc_inode`](libfs::table::alloc_inode).
pub fn alloc_inode(
    nod: INode,
    blocks: u8,
    fs_size: u64,
) -> Result<u64, AllocINodeError<FloppyError>> {
    table::alloc_inode(nod, blocks, fs_size, disk::write, &INODE_BMP, &INODE_TBL)
}

/// See [`read_inode`](libfs::table::read_inode).
pub fn read_inode(ptr: u64, buf: &mut [u8]) -> Result<u16, InodeIOError<FloppyError>> {
    table::read_inode(ptr, buf, disk::read, &INODE_TBL)
}

/// Initialises and mounts the floppy filesystem.
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
        exit_on_err!(init::reformat_drive(&GOOD_FS_HEADER, disk::write))
    }

    // Check if the filesystem is a newer version
    let fs_release = fsheader.release();
    if fs_release > GOOD_FS_HEADER.release() {
        dbg_info!("Filesystem has newer release than kernel, some features may not be supported")
    }

    let feats = fsheader.features;
    dbg_info!(
        "Found floppy filesystem: {}, released {fs_release}\nFilesystem features: {feats}",
        str::from_utf8(&fsheader.name).unwrap_or("filesystem contains bad name"),
    );

    let _active = exit_on_err!(init::read_table(feats, disk::read, &INODE_BMP, &INODE_TBL));
    dbg_info!("Read inode table, active inodes: {_active}");
    FLOPPYFS_INIT.store(true, Ordering::Relaxed);
    ExitCode::Ok
}

/// An error created when trying to initialise the floppy filesystem.
#[derive(Error, Debug)]
pub enum InitError {
    #[error("the floppy driver failed!")]
    NoFloppyDriver,

    #[error("floppy error: {0}")]
    FloppyError(#[from] FloppyError),

    #[error("the floppy drive was corrupt!")]
    CorruptDrive,

    #[error("read table error: {0}")]
    TableError(#[from] ReadTableError<FloppyError>),
}
