//! Utility functions used to help initialise a filesystem.

use super::{
    BLOCK_SIZE, FDC_CYL_SIZE, FilesystemFeatures, FilesystemHeader, INODE_START, INODES, INode,
    Read, Write,
    table::{self, InodeBitmap, InodeTable},
};
use core::mem;
use libutil::AsBytes;
use thiserror::Error;

/// Reformats the drive with a zeroed out inode table and `header` as it's filesystem header.
pub fn reformat_drive<E>(header: &FilesystemHeader, write: Write<E>) -> Result<(), E> {
    static BUF: [u8; INODES * size_of::<INode>()] = [0; INODES * size_of::<INode>()];
    write(0, header.as_bytes())?; // write new header

    // Zero out the inode table
    if header.features().contains(FilesystemFeatures::FLOPPY) {
        // FDC doesn't allow I/O over cylinder boundaries
        let cyl1_end = FDC_CYL_SIZE as usize - 1;
        write(INODE_START, &BUF[..cyl1_end * BLOCK_SIZE])?; // first cyl
        write(FDC_CYL_SIZE, &BUF[cyl1_end * BLOCK_SIZE..])?; // second cyl
    } else {
        write(INODE_START, &BUF)?
    }

    Ok(())
}

/// Reads the inode table into `tbl` and updates `bmp` accordingly,
/// returning the number of active inodes on success.
pub fn read_table<E>(
    feats: FilesystemFeatures,
    read: Read<E>,
    bmp: &InodeBitmap,
    tbl: &InodeTable,
) -> Result<u32, ReadTableError<E>> {
    // Read over the inode table
    let mut buf = [0; size_of::<INode>() * INODES];
    if feats.contains(FilesystemFeatures::FLOPPY) {
        // FDC doesn't allow I/O over cylinder boundaries
        let cyl1_end = FDC_CYL_SIZE as usize - 1;
        read(INODE_START, &mut buf[..cyl1_end * BLOCK_SIZE])?; // first cyl
        read(FDC_CYL_SIZE, &mut buf[cyl1_end * BLOCK_SIZE..])?; // second cyl
    } else {
        read(INODE_START, &mut buf)?
    }

    // Safety: All bit patterns of inode are safe
    let nods = unsafe { mem::transmute::<[u8; size_of::<INode>() * INODES], [INode; INODES]>(buf) };
    let mut active_inodes = 0u32;

    // Update the memory-based table and bitmap, one inode at a time
    for (idx, map) in tbl.iter().enumerate() {
        let inode = nods[idx].clone();
        let (links, meta) = (inode.links, inode.meta.clone());
        map.map(|v| *v = inode).ok_or(ReadTableError::ExmapError)?;

        if links != 0 {
            active_inodes += 1;

            // Update free block bitmap
            for ptrs in meta.iter() {
                for ptr in ptrs.decode().into_iter().filter(|p| *p != 0) {
                    table::alloc_bmp(ptr as u64, bmp).ok_or(ReadTableError::ExmapError)?;
                }
            }
        }
    }

    Ok(active_inodes)
}

/// An error created when trying to read over the inode table.
#[derive(Error, Debug)]
pub enum ReadTableError<E> {
    #[error("read fn error: {0}")]
    ReadError(#[from] E),

    #[error("unable to access an exclusive map value")]
    ExmapError,
}
