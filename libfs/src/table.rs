//! Handles the inode table.
//! Most functions operate on [`InodeTable`] & [`BlockBitmap`] values, which can be created as statics via the [`table_statics!`] macro.

use crate::{BlockPtr, INODE_START, INODES, INode, InodePtr, Write};
use libutil::{AsBytes, ExclusiveMap};
use thiserror::Error;

/// Memory cached inode table written to disk on write.
pub type InodeTable = [ExclusiveMap<INode>; INODES];

/// Bitmap of freely available blocks, where a zero indicates that a block is available.
pub type BlockBitmap = [ExclusiveMap<u128>; BlockPtr::MAX_VAL as usize / U128_BITS];

/// The number of bits in a `u128`.
const U128_BITS: usize = u128::BITS as usize;

/// Creates the required [`InodeTable`] & [`BlockBitmap`] statics.
/// Note: These statics take up just over 20 KB in memory.
#[macro_export]
macro_rules! table_statics {
    () => {
        static INODE_TBL: InodeTable = [const { ExclusiveMap::new(INode::zeroed()) }; INODES];
        static BLOCK_BMP: BlockBitmap =
            [const { ExclusiveMap::new(0) }; BlockPtr::MAX_VAL as usize / u128::BITS as usize];
    };
}

/// Tries to mark the block `block` in the [`BlockBitmap`] as used if it's available.
pub fn alloc_bmp(block: &BlockPtr, bmp: &BlockBitmap) -> Result<(), AllocBmpError> {
    let ptr = block.get().ok_or(AllocBmpError::NullPtr)? as usize;
    let idx = ptr / U128_BITS;
    let bit = 1u128 << (ptr % U128_BITS);

    match bmp[idx].map(|i| {
        // Check if the bit is already set
        if *i & bit != 0 {
            Err(AllocBmpError::AlreadyInUse(block.clone()))
        } else {
            *i |= bit; // set bit
            Ok(())
        }
    }) {
        Some(res) => res,
        None => Err(AllocBmpError::ExmapInUse(block.clone())),
    }
}

/// The error returned from [`alloc_bmp`].
#[derive(Error, Debug, PartialEq)]
pub enum AllocBmpError {
    #[error("attempted allocating a null block ptr")]
    NullPtr,

    #[error("{0} is already allocated")]
    AlreadyInUse(BlockPtr),

    #[error("{0}'s exmap is being used somewhere else")]
    ExmapInUse(BlockPtr),
}

/// Allocates the next available block in the block bitmap,
/// returning a null ptr if the bitmap is full.
pub fn alloc_next_bmp(bmp: &BlockBitmap) -> BlockPtr {
    for ptr in 1..BlockPtr::MAX_VAL {
        let blk = BlockPtr::new(ptr);
        if alloc_bmp(&blk, bmp).is_ok() {
            return blk;
        }
    }

    BlockPtr::null()
}

/// Tries to allocate inode `nod` in `tbl`, returning a pointer to it.
pub fn alloc_inode<E>(
    nod: &INode,
    tbl: &InodeTable,
    write: Write<E>,
) -> Result<InodePtr, AllocInodeError<E>> {
    for (idx, exmap) in tbl.iter().enumerate() {
        if let Some(alloc) = exmap.map(|n| {
            let alloc = n.is_available();
            if alloc {
                *n = nod.clone() // update table
            };
            alloc
        }) && alloc
        {
            // we found a nod!
            let ptr = InodePtr::new(idx as u16 + 1); // add one so inode 0 isn't seen as a null pointer
            write_inode_block(&ptr, tbl, write)?;
            return Ok(InodePtr::new(idx as u16));
        }
    }

    Err(AllocInodeError::OutOfInodes)
}

#[derive(Error, Debug)]
pub enum AllocInodeError<E> {
    #[error("ran out of space on the inode table!")]
    OutOfInodes,

    #[error("update error: {0}")]
    UpdateInode(#[from] UpdateInodeError<E>),
}

/// Writes the non-null inode pointer `ptr`, as well as the other inodes in it's block to the drive.
fn write_inode_block<E>(
    ptr: &InodePtr,
    tbl: &InodeTable,
    write: Write<E>,
) -> Result<(), UpdateInodeError<E>> {
    let ptr = ptr.get_table_idx().ok_or(UpdateInodeError::NullPtr)? as u64;
    let block = (ptr & !0b11) + INODE_START; // round down to start of block

    // Get nods in the block
    let (mut buf, ptr) = ([const { INode::zeroed() }; 4], ptr as usize);
    for (idx, exmap) in tbl[ptr..ptr + 4].iter().enumerate() {
        if exmap.map(|n| buf[idx] = n.clone()).is_none() {
            return Err(UpdateInodeError::TblExmapFailure);
        }
    }

    write(block, buf.as_bytes())?;
    Ok(())
}

#[derive(Error, Debug)]
pub enum UpdateInodeError<E> {
    #[error("passed a null pointer")]
    NullPtr,

    #[error("unable to access nods in the table exmap")]
    TblExmapFailure,

    #[error("write error: {0}")]
    WriteError(#[from] E),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FileMode;

    /// Tests that [`alloc_bmp`] prevents allocating the same block twice.
    #[test]
    #[allow(unused)]
    #[rustfmt::skip]
    fn no_double_bmp_alloc() {
        table_statics!();
        assert_eq!(alloc_bmp(&BlockPtr(None), &BLOCK_BMP), Err(AllocBmpError::NullPtr));
        for ptr in 1..BlockPtr::MAX_VAL {
            let blk = BlockPtr::new(ptr);
            assert_eq!(alloc_bmp(&blk, &BLOCK_BMP), Ok(()));
            assert_eq!(alloc_bmp(&blk, &BLOCK_BMP), Err(AllocBmpError::AlreadyInUse(blk)));
        }
    }

    /// Tests that [`alloc_bmp`] sees writes to the block bitmap.
    #[test]
    #[allow(unused)]
    #[rustfmt::skip]
    fn bmp_alloc_persistence() {
        table_statics!();
        BLOCK_BMP[0].map(|idx| *idx = 0b10).unwrap(); // alloc block 1
        let blk = BlockPtr::new(1);
        assert_eq!(alloc_bmp(&blk, &BLOCK_BMP), Err(AllocBmpError::AlreadyInUse(blk)));
        assert_eq!(alloc_bmp(&BlockPtr::new(2), &BLOCK_BMP), Ok(()));
    }

    /// Tests that [`alloc_inode`] works ok.
    #[test]
    #[allow(unused)]
    fn alloc_inode_works() {
        table_statics!();
        static NOD: INode = INode::new(FileMode::DIRECTORY, 0, InodePtr::new(0x42));
        fn write(ptr: u64, buf: &[u8]) -> Result<(), ()> {
            assert_eq!(ptr, 1); // writing to the first inode block
            assert_eq!(&buf[..size_of::<INode>()], NOD.as_bytes()); // nod wrote ok
            assert_eq!(&buf[size_of::<INode>()..], [0; size_of::<INode>() * 3]); // the rest of the buf is uninit nods
            Ok(())
        }

        alloc_inode(&NOD, &INODE_TBL, write).unwrap();
        INODE_TBL[0].map(|n| assert_eq!(*n, NOD));
    }

    /// Tests that [`alloc_next_bmp`] works correctly.
    #[test]
    #[allow(unused)]
    fn alloc_next_bmp_works() {
        table_statics!();
        for ptr in 1..BlockPtr::MAX_VAL {
            assert_eq!(alloc_next_bmp(&BLOCK_BMP), BlockPtr::new(ptr));
        }
        assert!(alloc_next_bmp(&BLOCK_BMP).is_null())
    }

    /// Tests that [`InodeTable`] & [`BlockBitmap`] have the right size.
    #[test]
    #[rustfmt::skip]
    fn types_have_the_right_size() {
        assert_eq!(size_of::<InodeTable>(), size_of::<ExclusiveMap<INode>>() * INODES);
        assert_eq!(size_of::<BlockBitmap>(), size_of::<ExclusiveMap<u128>>() * BlockPtr::MAX_VAL as usize / U128_BITS);
    }
}
