//! Utility functions used to help initialise a filesystem.

use super::{
    INODE_START, INODES, INode, Read, Write,
    table::{BlockBitmap, InodeTable},
};
use crate::{
    header::FilesystemHeader,
    table::{self, AllocBmpError},
};
use core::mem;
use libutil::AsBytes;
use thiserror::Error;

/// Reformats the drive with a zeroed out inode table and `header` as it's filesystem header.
pub fn reformat_drive<E>(header: &FilesystemHeader, write: Write<E>) -> Result<(), E> {
    static BUF: [u8; INODES * size_of::<INode>()] = [0; INODES * size_of::<INode>()];
    write(0, header.as_bytes())?; // write new header
    write(INODE_START, &BUF)?; // zero out the inode table

    Ok(())
}

/// Reads the inode table into `tbl` and updates `bmp` accordingly,
/// returning the number of active inodes and used blocks on success.
pub fn read_table<E>(
    tbl: &InodeTable,
    bmp: &BlockBitmap,
    read: Read<E>,
) -> Result<(u32, u32), ReadTblError<E>> {
    // Read over the inode table
    let mut buf = [0; size_of::<INode>() * INODES];
    read(INODE_START, &mut buf)?;

    // Safety: All bit patterns of inode are safe
    let nods = unsafe { mem::transmute::<[u8; size_of::<INode>() * INODES], [INode; INODES]>(buf) };
    let (mut active_nods, mut used_blocks) = (0, 0);

    // Update table & bitmap, skipping uninit nods
    for (idx, nod) in nods.iter().filter(|n| !n.is_available()).enumerate() {
        active_nods += 1;
        if tbl[idx].map(|n| *n = nod.clone()).is_none() {
            return Err(ReadTblError::ExmapError);
        }

        // Update valid blocks to bmp
        for blks in nod.blocks.iter().map(|b| b.decode()) {
            for blk in blks.iter().filter(|b| b.is_valid()) {
                table::alloc_bmp(blk, bmp).map_err(|e| ReadTblError::AllocBmp(e))?;
                used_blocks += 1;
            }
        }
    }

    Ok((active_nods, used_blocks))
}

/// An error created when trying to read over the inode table.
#[derive(Error, Debug)]
pub enum ReadTblError<E> {
    #[error("read error: {0}")]
    ReadError(#[from] E),

    #[error("unable to access an exmap value")]
    ExmapError,

    #[error("alloc bmp error: {0}")]
    AllocBmp(AllocBmpError),
}
