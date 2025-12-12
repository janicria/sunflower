//! Handles the inode table.
//! Expects [`InodeTable`] & [`InodeBitmap`] statics, which can be created through the [`inode_statics`] macro.

use super::{BLOCK_SIZE, BLOCK_START, DualBlockPtr, INODE_START, INODES, INode, Read, Write};
use libutil::ExclusiveMap;
use thiserror::Error;

/// Memory cached inode table written to disk on write.
pub type InodeTable = [ExclusiveMap<INode>; INODES];

/// Bitmap of freely available blocks, where a zero indicates that a block is available.
pub type InodeBitmap = [ExclusiveMap<u8>; INODES * 8];

/// Checking if inodes and blocks are available can spuriously fail due to the use of exmaps.
const EXMAP_RETRIES: u8 = 8;

/// Creates the required [`InodeTable`] & [`InodeBitmap`] statics.
#[macro_export]
macro_rules! inode_statics {
    () => {
        static INODE_TBL: InodeTable = [const { ExclusiveMap::new(INode::zeroed()) }; INODES];
        static INODE_BMP: InodeBitmap = [const { ExclusiveMap::new(0) }; INODES * 8];
    };
}

/// Marks the `ptr`th block in the [`InodeBitmap`] as used, then returns it's previous value.
///
/// Returns `None` if the bitmap can't be accessed.
pub fn alloc_bmp(ptr: u64, bmp: &InodeBitmap) -> Option<bool> {
    let byte = (ptr as usize) / 8;
    let bit = 7 - (ptr % 8);

    bmp[byte].map(|i| {
        let prev = (*i >> bit) & 1 == 1; // is bit set?
        *i |= 1 << bit; // set bit
        prev
    })
}

/// Tries to allocate inode `nod` in the first available slot, not passing block `fs_size`, in the table with `blocks` blocks.
/// Always sets the `links` field on the `INode` to 1.
/// 
/// Returns a pointer to the newly allocated inode it on success.
pub fn alloc_inode<E>(
    nod: INode,
    blocks: u8,
    fs_size: u64,
    write: Write<E>,
    bmp: &InodeBitmap,
    tbl: &InodeTable,
) -> Result<u64, AllocINodeError<E>> {
    let blocks = blocks.min(64) as usize; // inodes can only store up to 64 ptrs
    let mut err = AllocINodeError::OutOfInodes;

    for _ in 0..EXMAP_RETRIES {
        let mut alloc_blks = 0; // the number of blocks we've allocated
        let mut use_ptr2 = false; // do we use the first or second ptr for the next block?
        let mut block_ptrs = DualBlockPtr::empty_arr();

        // Find those blocks!
        for ptr in BLOCK_START..fs_size {
            if alloc_blks == blocks {
                break; // we've allocated the right amount of blocks!
            }

            if let Some(used) = alloc_bmp(ptr, bmp)
                && !used
            {
                // We found an available block!
                let mut ptrs = block_ptrs[alloc_blks].decode();
                ptrs[use_ptr2 as usize] = ptr as u16;
                block_ptrs[alloc_blks] = DualBlockPtr::encode(ptrs);

                use_ptr2 = !use_ptr2;
                alloc_blks += 1;
            }
        }

        // Check if we found enough blocks to allocate
        if blocks != 0 && block_ptrs[blocks - 1].decode()[0] == 0 {
            err = AllocINodeError::OutOfBlocks;
            break;
        }

        // Try find an inode
        err = AllocINodeError::OutOfInodes;
        for (idx, exmap) in tbl.iter().enumerate() {
            let mut inode_ptr = None;
            if let Some(()) = exmap.map(|inode| {
                if inode.links == 0 {
                    // We found an available inode!
                    inode.mode = nod.mode;
                    inode.links = 1;
                    inode.size = nod.size;
                    inode.meta = block_ptrs.clone(); // annoying clone because compiler sucks
                    inode_ptr = Some(idx as u64);
                }
            }) && let Some(ptr) = inode_ptr
            {
                write_inode(ptr, write, tbl)?;
                return Ok(ptr);
            };
        }
    }

    Err(err)
}

/// The error returned when trying to initialise an inode.
#[derive(Error, Debug)]
pub enum AllocINodeError<E> {
    #[error("ran out of available inodes, delete some files to regain entries")]
    OutOfInodes,

    #[error("ran out of blocks on the filesystem, looks like all storage has been used up")]
    OutOfBlocks,

    #[error(transparent)]
    WriteError(#[from] InodeIOError<E>),
}

/// Writes the `ptr`th inode to disk. May block for up to 20 ms.
fn write_inode<E>(ptr: u64, write: Write<E>, tbl: &InodeTable) -> Result<(), InodeIOError<E>> {
    let mut buf = [const { INode::zeroed() }; 4];
    let lba = (ptr as usize) & !0b11; // lowest multiple of 4

    if lba > INODES {
        return Err(InodeIOError::NoInodeFound(ptr));
    }

    // Try to read from the table
    'retries: for _ in 0..EXMAP_RETRIES {
        for (idx, inode) in tbl[lba..lba + 4].iter().enumerate() {
            if inode.map(|nod| buf[idx] = nod.clone()).is_none() {
                continue 'retries;
            }
        }

        let buf: [u8; size_of::<INode>() * 4] = unsafe { core::mem::transmute(buf) };
        return write(INODE_START + lba as u64, &buf).map_err(Into::into);
    }

    Err(InodeIOError::TableBusy)
}

/// Reads the data in `ptr`th inode from disk `buf` and returns the number of blocks read.
/// May block for up to 20 ms.
pub fn read_inode<E>(
    ptr: u64,
    buf: &mut [u8],
    read: Read<E>,
    tbl: &InodeTable,
) -> Result<u16, InodeIOError<E>> {
    let exmap = tbl
        .get(ptr as usize)
        .ok_or(InodeIOError::NoInodeFound(ptr))?;

    // Try to read from the table
    let mut ptrs = DualBlockPtr::empty_arr();
    for _ in 0..EXMAP_RETRIES {
        if exmap.map(|nod| ptrs = nod.meta.clone()).is_none() {
            continue;
        }

        // Ok! ptrs now contains the blocks we need to read from
        let mut ptrs_read = 0;
        let mut tmp_buf = [0; BLOCK_SIZE];

        // Read the ptrs into the buf one block at a time, filtering out null ptrs
        for ptrs in ptrs.iter().map(|p| p.decode()) {
            for ptr in ptrs.into_iter().filter(|ptr| *ptr != 0) {
                // Return if we've hit the end of the supplied buffer
                if buf.len() < (ptrs_read + 1) * BLOCK_SIZE {
                    return Ok(ptrs_read as u16);
                }

                read(ptr as u64, &mut tmp_buf)?;
                buf[ptrs_read * BLOCK_SIZE..(ptrs_read + 1) * BLOCK_SIZE].copy_from_slice(&tmp_buf);
                ptrs_read += 1;
            }
        }

        // Return now that we've read all the ptrs
        return Ok(ptrs_read as u16);
    }

    Err(InodeIOError::TableBusy)
}

/// The error returned when trying to write and write inodes to/from disk.
#[derive(Error, Debug)]
pub enum InodeIOError<E> {
    #[error("the inode task couldn't be accessed in a reasonable amount of time")]
    TableBusy,

    #[error("write fn error: {0}")]
    WriteError(#[from] E),

    #[error("no inode found with index {0}")]
    NoInodeFound(u64),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that [`alloc_bmp`] works correctly.
    #[test]
    #[allow(unused)]
    fn alloc_bmp_works() {
        inode_statics!();
        for ptr in 0..INODES as u64 {
            assert_eq!(alloc_bmp(ptr, &INODE_BMP), Some(false));
            assert_eq!(alloc_bmp(ptr, &INODE_BMP), Some(true));
        }
    }

    /// Tests that [`InodeTable`] & [`InodeBitmap`] have the right size.
    #[test]
    #[rustfmt::skip]
    fn types_have_the_right_size() {
        assert_eq!(size_of::<InodeTable>(), size_of::<ExclusiveMap<INode>>() * INODES);
        assert_eq!(size_of::<InodeBitmap>(), size_of::<ExclusiveMap<u8>>() * INODES * u8::BITS as usize);
    }
}
