//!  Filesystem layout
//!
//! - Block 0 - Filesystem header
//! - Block 1-36 - Inode table
//! - Block 37... - Regular file data

use bitflags::bitflags;
use core::{fmt::Display, mem};
pub use floppyfs::{FLOPPYFS_INIT, alloc_inode, init_floppyfs, read_inode};
use libutil::AsBytes;

/// A floppy disk connected filesystem.
mod floppyfs;

/// The filesystem magic number
pub static MAGIC: [u8; 6] = [0xFD, b'S', b'F', b'K', 0x86, 0x64];

/// The number of USABLE inodes in the inode table.
const INODES: usize = 140; // header + inodes fit exactly in two cylinders of the floppy

/// The linear block address of the start of the inode table.
const INODE_START: u16 = 1;

/// The linear block address of the start of blocks usable by inode meta.
const BLOCK_START: u16 = INODE_START + (INODES / INODES_PER_BLOCK) as u16 + 1;

/// The number of inodes contained in a block.
const INODES_PER_BLOCK: usize = 4;

/// The metadata for a file, stored in the inode table.
#[repr(C, packed)]
struct INode {
    /// The file's type & permissions.
    mode: FileMode,

    /// The number of links to the file.
    links: u8,

    /// The size of the file, in bytes.
    /// The sign bit is ignored, allowing up to 2^15/1024 = 32 KiB files.
    size: i16,

    /// Direct pointers to the blocks used by the file.
    /// For regular files, supports:
    /// * the full range of file sizes as 32 \* 2 \* 512 / 1024 = 32 KiB,
    ///
    /// for directories:
    /// * up to 64 \* 512 / 32 = 1024 child inodes.
    meta: [DualBlockPtr; 32],

    // reserved for future use,
    // will eventually become uid, gid and various time fields
    _reserved: [u8; 27],
}

bitflags! {
    #[derive(Clone, Copy)]
    /// The type and permissions for a file.
    pub struct FileMode: u16 {
        /// Set if the inode is being used by a file.
        const ACTIVE = 1;

        const _ = !0;
    }
}

/// Represents two block pointers, as a single block pointer is only 12 bits.
#[derive(Clone)]
#[repr(transparent)]
struct DualBlockPtr([u8; 3]);

/// Represents a 30 byte file name and a 2 byte inode index.
#[allow(unused)]
#[repr(C)]
struct FileLookup {
    name: [u8; 30],
    inode: u16,
}

/// The first sector on the filesystem.
#[repr(C, packed)]
struct FilesystemHeader {
    /// The filesystem magic number, should be `0xFD SFK x86 x64` (0xFD53464B8664)
    magic: [u8; 6],

    /// When the filesystem was last updated in UTC
    /// - Bits 0-9 - the day of the year (Jan 1st = 1)
    /// - Bits 10-15 - the number of years since 2025
    release: u16,

    /// The features available on the filesystems version.
    features: FilesystemFeatures,

    /// The name of the filesystem.
    name: [u8; 16],

    /// Where the filesystem should be mounted relative to /
    mountpoint: [u8; 64],

    // reserved to reach a size of 512 bytes, or one block
    _reserved: [u8; 416],
}

bitflags! {
    struct FilesystemFeatures: u64 {
        /// The filesystem is connected to a floppy drive.
        const FLOPPY = 1;

        const _ = !0;
    }
}

impl INode {
    // Returns an empty inode.
    const fn zeroed() -> Self {
        INode {
            mode: FileMode::empty(),
            links: 0,
            size: 0,
            meta: [const { DualBlockPtr([0; 3]) }; 32],
            _reserved: [0; 27],
        }
    }

    /// Returns a copy of the inode's mode.
    fn mode(&self) -> FileMode {
        self.mode
    }
}

impl Clone for INode {
    fn clone(&self) -> Self {
        INode {
            mode: self.mode,
            links: self.links,
            size: self.size,
            meta: self.meta.clone(),
            _reserved: [0; 27],
        }
    }
}

impl Display for INode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let (mode, size) = (self.mode.0, self.size);
        write!(f, "inode {mode:?} {} {size}b {{", self.links)?;
        for ptrs in self.meta.iter() {
            ptrs.fmt(f)?
        }
        write!(f, "}}")
    }
}

impl DualBlockPtr {
    /// Returns the two block pointers.
    fn decode(&self) -> [u16; 2] {
        let first = ((self.0[0] as u16) << 4) | ((self.0[1] as u16 & 0b1111_0000) >> 4);
        let second = ((self.0[1] as u16 & 0b0000_1111) << 8) | (self.0[2] as u16);
        [first, second]
    }

    /// Creates a new dual pointer struct from the two pointers.
    fn encode(ptrs: [u16; 2]) -> Self {
        let one = (ptrs[0] >> 4) as u8;
        let two = (((ptrs[0] & 0b1111) << 4) | (ptrs[1] & 0b1111_0000_0000) >> 8) as u8;
        let three = ptrs[1] as u8;
        DualBlockPtr([one, two, three])
    }
}

impl Display for DualBlockPtr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for ptr in self.decode().iter().filter(|p| **p != 0) {
            write!(f, "{ptr}")?
        }
        Ok(())
    }
}

impl FilesystemHeader {
    /// Converts an array of bytes into a header.
    fn from_raw(bytes: [u8; size_of::<FilesystemHeader>()]) -> Self {
        // Safety: All bit patterns of filesystem header are valid
        unsafe { mem::transmute::<[u8; size_of::<FilesystemHeader>()], FilesystemHeader>(bytes) }
    }
}

// Safety: Both types are packed never containing any uninit bytes or interior mutability.
unsafe impl AsBytes for INode {}
unsafe impl AsBytes for FilesystemHeader {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that dual block pointers are correctly encode and decoded.
    #[test_case]
    fn dual_block_ptr_encoding() {
        for fst in 0..2u16.pow(12) {
            for snd in 0..2u16.pow(12) {
                let decoded = DualBlockPtr::encode([fst, snd]).decode();
                assert_eq!(fst, decoded[0]);
                assert_eq!(snd, decoded[1])
            }
        }
    }
}
