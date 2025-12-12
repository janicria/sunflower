//! Library for creating, reading and modifying sunflower readable filesystems.
//!
//! This library is intended to be used in conjunction with two external functions defined as
//! ```should_fail
//! fn read<E>(sector: u64, buf: &mut [u8]) -> Result<(), E>
//! fn write<E>(sector: u64, buf: &[u8]) -> Result<(), E>
//! ```
//!
//!
//!  Filesystem layout:
//! - Block 0 - Filesystem header
//! - Block 1-36 - Inode table
//! - Block 37... - Regular file data

#![no_std]

use bitflags::bitflags;
use core::{cmp::Ordering, fmt::Display, mem};
use libutil::AsBytes;

#[macro_use]
pub mod table;
pub mod init;

pub type Read<E> = fn(sector: u64, buf: &mut [u8]) -> Result<(), E>;
pub type Write<E> = fn(sector: u64, buf: &[u8]) -> Result<(), E>;

/// The filesystem magic number
pub static MAGIC: [u8; 6] = [0xFD, b'S', b'F', b'K', 0x86, 0x64];

/// The number of USABLE inodes in the inode table.
pub const INODES: usize = 140; // header + inodes fit exactly one two cylinders on the floppy

/// The linear block address of the start of the inode table.
pub const INODE_START: u64 = 1;

/// The linear block address of the start of blocks usable by inode meta.
pub const BLOCK_START: u64 = INODE_START + (INODES as u64 / INODES_PER_BLOCK) + 1;

/// The number of inodes contained in a block.
pub const INODES_PER_BLOCK: u64 = (BLOCK_SIZE / size_of::<INode>()) as u64;

/// The numbers of bytes in a block;
pub const BLOCK_SIZE: usize = 512;

/// The number of sectors / blocks in each cylinder of the floppy.
pub const FDC_CYL_SIZE: u64 = 18;

/// The metadata for a file, stored in the inode table.
#[repr(C, packed)]
pub struct INode {
    /// The file's type & permissions.
    mode: FileMode,

    /// The number of links to the file.
    links: u8,

    /// The size of the file, in bytes.
    /// The sign bit is ignored, allowing up to 2^15/1024 = 32 KiB files.
    size: i16,

    /// Direct pointers to the blocks used by the file.
    /// For regular files, supports:
    /// * the full range of file sizes as 64 \* 512 / 1024 = 32 KiB,
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
        const _ = !0;
    }
}

/// Represents two block pointers, as a single block pointer is only 12 bits.
#[derive(Clone)]
#[repr(transparent)]
pub struct DualBlockPtr([u8; 3]);

/// Represents a 30 byte file name and a 2 byte inode index.
#[repr(C)]
pub struct FileLookup {
    name: [u8; 30],
    inode: u16,
}

/// The first sector on the filesystem.
#[repr(C, packed)]
pub struct FilesystemHeader {
    /// The filesystem magic number, should be `0xFD SFK x86 x64` (0xFD53464B8664)
    pub magic: [u8; 6],

    /// When the filesystem was last updated
    pub release: FsRelease,

    /// The features available on the filesystems version.
    pub features: FilesystemFeatures,

    /// The name of the filesystem.
    pub name: [u8; 16],

    /// Where the filesystem should be mounted relative to /
    pub mountpoint: [u8; 64],

    /// The size of the filesystem in blocks.
    pub size: u64,

    // reserved to reach a size of 512 bytes, or one block
    _reserved: [u8; 408],
}

/// Represents when a filesystem was last updated in UTC
/// - Bits 0-9 - the day of the year (Jan 1st = 1)
/// - Bits 10-15 - the number of years since 2025
#[derive(PartialEq, Clone, Copy)]
#[repr(transparent)]
pub struct FsRelease(u16);

bitflags! {
    #[derive(Clone, Copy)]
    pub struct FilesystemFeatures: u64 {
        /// The filesystem is connected to a floppy drive.
        const FLOPPY = 1;

        const _ = !0;
    }
}

impl INode {
    // Returns an empty inode.
    pub const fn zeroed() -> Self {
        INode {
            mode: FileMode::empty(),
            links: 0,
            size: 0,
            meta: DualBlockPtr::empty_arr(),
            _reserved: [0; 27],
        }
    }

    /// Creates a new inode.
    pub const fn new(mode: FileMode, size: i16) -> Self {
        INode {
            mode,
            links: 1,
            size,
            meta: DualBlockPtr::empty_arr(),
            _reserved: [0; 27],
        }
    }

    /// Returns a copy of the inode's mode.
    pub fn mode(&self) -> FileMode {
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
    /// Returns an empty set of ptrs for an inode.
    pub const fn empty_arr() -> [DualBlockPtr; 32] {
        [const { DualBlockPtr([0; 3]) }; 32]
    }

    /// Returns the two block pointers.
    pub fn decode(&self) -> [u16; 2] {
        let first = ((self.0[0] as u16) << 4) | ((self.0[1] as u16 & 0b1111_0000) >> 4);
        let second = ((self.0[1] as u16 & 0b0000_1111) << 8) | (self.0[2] as u16);
        [first, second]
    }

    /// Creates a new dual pointer struct from the two pointers.
    pub fn encode(ptrs: [u16; 2]) -> Self {
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
    /// Creates a new fsheader from the given fields.
    pub const fn new(
        name: [u8; 16],
        day: u16,
        year: u16,
        mountpoint: [u8; 64],
        size: u64,
        feats: FilesystemFeatures,
    ) -> FilesystemHeader {
        FilesystemHeader {
            magic: MAGIC,
            release: FsRelease::new(day, year),
            features: feats,
            name,
            mountpoint,
            size,
            _reserved: [0; 408],
        }
    }

    /// Converts an array of bytes into a header.
    pub fn from_raw(bytes: [u8; size_of::<FilesystemHeader>()]) -> Self {
        // Safety: All bit patterns of filesystem header are valid
        unsafe { mem::transmute::<[u8; size_of::<FilesystemHeader>()], FilesystemHeader>(bytes) }
    }

    /// Returns a copy of the header's features.
    pub fn features(&self) -> FilesystemFeatures {
        self.features
    }

    /// Returns a copy of the header's release.
    pub fn release(&self) -> FsRelease {
        self.release
    }
}

impl FsRelease {
    /// The lowest possible year.
    const YEAR_START: u16 = 2025;

    /// Creates a new release.
    /// Subtracts 2025 from the year.
    pub const fn new(day: u16, year: u16) -> FsRelease {
        FsRelease(((year - Self::YEAR_START) << 10) | day)
    }

    /// Returns the `(year, day)` components of the release.
    /// Doesn't add 2025 to the year.
    pub fn year_day(&self) -> (u16, u16) {
        let yr = self.0 >> 10;
        let day = self.0 & 0b111111111;
        (yr, day)
    }
}

impl PartialOrd for FsRelease {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        if self == other {
            Some(Ordering::Equal)
        } else {
            let (self_yr, self_day) = self.year_day();
            let (other_yr, other_day) = other.year_day();

            if other_yr > self_yr || (self_yr == other_yr && other_day > self_day) {
                Some(Ordering::Less)
            } else {
                Some(Ordering::Greater)
            }
        }
    }
}

impl Display for FsRelease {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let (year, day) = self.year_day();
        write!(f, "{day}:{}", year + Self::YEAR_START)
    }
}

impl Display for FilesystemFeatures {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// Safety: Both types are packed never containing any uninit bytes or interior mutability.
unsafe impl AsBytes for INode {}
unsafe impl AsBytes for FilesystemHeader {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that various types have the correct size.
    #[test]
    fn types_have_the_right_size() {
        assert_eq!(size_of::<INode>(), 128);
        assert_eq!(size_of::<FilesystemHeader>(), 512);
    }

    /// Tests that dual block pointers are correctly encoded and decoded.
    #[test]
    fn dual_block_ptr_encoding() {
        for fst in 0..2u16.pow(12) {
            for snd in 0..2u16.pow(12) {
                let decoded = DualBlockPtr::encode([fst, snd]).decode();
                assert_eq!(fst, decoded[0]);
                assert_eq!(snd, decoded[1])
            }
        }
    }

    /// Tests that filesystem releases are correctly encoded and decoded.
    #[test]
    fn fs_release_encoding() {
        for year in 0..2u16.pow(6) {
            for day in 0..2u16.pow(10) {
                let (yr, day) = FsRelease::new(day, FsRelease::YEAR_START + year).year_day();
                assert_eq!(yr, year);
                assert_eq!(day, day);
            }
        }
    }
}
