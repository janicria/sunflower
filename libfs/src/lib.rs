/* ---------------------------------------------------------------------------
    libfs - Sunflower kernel filesystem library, sunflowerkernel.org
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
    libfs/src/lib.rs

    Library root file

    This library is intended to be used with two external functions defined as
    ```rust
    /// Read starting from block `block` on the drive into `buf` until it's full.
    pub type Read<E> = fn(block: u64, buf: &mut [u8]) -> Result<(), E>;

    /// Write `buf` to the drive starting at block `block`.
    pub type Write<E> = fn(block: u64, buf: &[u8]) -> Result<(), E>;
    ```

    Note: Blocks are 512 bytes in length.

    Filesystem layout:
     - Block 0 - Filesystem header
    - Block 1-36 - Inode table
    - Block 37... - File data used by inodes
*/

//!
//!     ----- IMPORTANT!!!!! -----
//! 
//!     this entire library will be removed soon (probs next patch)
//!     when I finish designing the Snug Filesystem

#![no_std]

use bitflags::bitflags;
use core::{
    fmt::{Debug, Display},
    num::NonZero,
};
use libutil::AsBytes;

#[macro_use]
pub mod table;
pub mod header;
pub mod init;

/// Read starting from block `block` on the drive into `buf` until it's full.
pub type Read<E> = fn(block: u64, buf: &mut [u8]) -> Result<(), E>;

/// Write `buf` to the drive starting at block `block`.
pub type Write<E> = fn(block: u64, buf: &[u8]) -> Result<(), E>;

/// The filesystem magic number
pub static MAGIC: [u8; 6] = [0xFD, b'S', b'F', b'K', 0x86, 0x64];

/// The number of USABLE inodes in the inode table.
pub const INODES: usize = 140; // header + inodes fit exactly one two cylinders on the floppy

/// The linear block address of the start of the inode table.
pub const INODE_START: u64 = 1;

/// The linear block address of the start of blocks usable by inode meta.
// Actually the last inode block, yet is unreachable due to block ptrs always being > 0
pub const BLOCK_START: u64 = INODE_START + (INODES / 4) as u64;

/// The numbers of bytes in a block;
pub const BLOCK_SIZE: usize = 512;

/// The metadata for a file, stored in the inode table.
#[repr(C, packed)]
pub struct INode {
    /// The inode's type & permissions.
    mode: FileMode,

    /// The number of links to the inode.
    /// If an inode has zero links, it's considered available for use by other inodes.
    links: u8,

    /// The size of the file in bytes, allowing up to 2^16 = 64 KiB files.
    /// However inodes only actually support up to 24 KiB files.
    size: u16,

    /// Direct pointers to the blocks used by the file.
    /// For regular files, supports:
    /// * file sizes up to 24 \* 2 \* 512 = 24 KiB,
    ///
    /// for directories:
    /// * up to 24 \* 2 \* 512 / 32 = 768 child inodes.
    blocks: [DualBlockPtr; DualBlockPtr::INODE_PTRS],

    /// The inode's parent, can be null.
    parent: InodePtr,

    // reserved for future use,
    // will eventually become uid, gid and various time fields
    _reserved: [u8; Self::RESERVED_BYTES],
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
    /// The type and permissions for a file.
    pub struct FileMode: u16 {
        /// Set if the inode is a directory.
        const DIRECTORY = 1;

        const _ = !0;
    }
}

/// Represents two block pointers, as a single block pointer is only 12 bits.
#[derive(Debug, Clone, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct DualBlockPtr([u8; 3]);

/// A nullable pointer to an inode in the table.
// 16 bits for future compatibility when we have more than 140 inodes
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct InodePtr(Option<NonZero<u16>>);

/// What directory inode blocks are filled with.
/// Represents an inodes name and a pointer to it in the table
#[repr(C, packed)]
pub struct InodeLookup {
    name: [u8; Self::NAME_LEN],
    inode: InodePtr,
}

/// A nullable pointer to a block stored after the inode table.
/// The pointer is guaranteed to be `0 < ptr <= 2^12`.
#[derive(Debug, PartialEq, Clone)]
#[repr(transparent)]
pub struct BlockPtr(Option<NonZero<u16>>);

impl INode {
    /// The size of the `_reserved` field.
    const RESERVED_BYTES: usize = 49;

    // Returns an empty inode.
    pub const fn zeroed() -> Self {
        INode {
            mode: FileMode::empty(),
            links: 0,
            size: 0,
            parent: InodePtr::null(),
            blocks: DualBlockPtr::empty_arr(),
            _reserved: [0; Self::RESERVED_BYTES],
        }
    }

    /// Creates a new inode.
    pub const fn new(mode: FileMode, size: u16, parent: InodePtr) -> Self {
        INode {
            mode,
            links: 1,
            size,
            parent,
            blocks: DualBlockPtr::empty_arr(),
            _reserved: [0; Self::RESERVED_BYTES],
        }
    }

    /// Returns if the inode is available to be allocated.
    pub fn is_available(&self) -> bool {
        self.links == 0
    }

    /// Returns a copy of the inode's mode.
    pub fn mode(&self) -> FileMode {
        self.mode
    }

    /// Returns a copy of the inode's parent.
    pub fn parent(&self) -> InodePtr {
        self.parent
    }
}

// Safety: Inode is packed, never containing any uninit bytes nor interior mutability.
unsafe impl AsBytes for INode {}

impl Clone for INode {
    fn clone(&self) -> Self {
        INode {
            mode: self.mode,
            links: self.links,
            size: self.size,
            parent: self.parent,
            blocks: self.blocks.clone(),
            _reserved: [0; Self::RESERVED_BYTES],
        }
    }
}

impl PartialEq for INode {
    fn eq(&self, other: &Self) -> bool {
        // note: _reserved isn't checked
        self.size == other.size
            && self.parent() == other.parent()
            && self.links == other.links
            && self.mode() == other.mode()
            && self.blocks == other.blocks
    }
}

impl Debug for INode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let size = self.size;
        f.debug_struct("INode")
            .field("mode", &self.mode())
            .field("links", &self.links)
            .field("size", &size)
            .field("blocks", &self.blocks)
            .finish_non_exhaustive() // exclude _reserved
    }
}

impl Display for INode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let (mode, size) = (self.mode.0, self.size);
        write!(f, "inode {mode:?} {} {size}b {{", self.links)?;
        for ptrs in self.blocks.iter() {
            Display::fmt(ptrs, f)?
        }
        write!(f, "}}")
    }
}

impl DualBlockPtr {
    /// The number of dual block ptrs stored in an inode.
    /// Coincidentally also equals the number of KiB of data a inode can store.
    const INODE_PTRS: usize = 24;

    /// Returns an empty set of ptrs for an inode.
    pub const fn empty_arr() -> [DualBlockPtr; Self::INODE_PTRS] {
        [const { DualBlockPtr([0; 3]) }; Self::INODE_PTRS]
    }

    /// Returns the two block pointers.
    pub fn decode(&self) -> [BlockPtr; 2] {
        let first = ((self.0[0] as u16) << 4) | ((self.0[1] as u16 & 0b1111_0000) >> 4);
        let second = ((self.0[1] as u16 & 0b0000_1111) << 8) | (self.0[2] as u16);
        [BlockPtr::new(first), BlockPtr::new(second)]
    }

    /// Creates a new dual pointer struct from the two pointers.
    pub fn encode(ptrs: [&BlockPtr; 2]) -> Self {
        let (fst, snd) = (ptrs[0].get_nullable(), ptrs[1].get_nullable());

        let one = (fst >> 4) as u8;
        let two = (((fst & 0b1111) << 4) | (snd & 0b1111_0000_0000) >> 8) as u8;
        let three = snd as u8;

        DualBlockPtr([one, two, three])
    }
}

impl Display for DualBlockPtr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let ptrs = self.decode();
        write!(f, "({}, {})", ptrs[0], ptrs[1])
    }
}

impl InodePtr {
    /// Returns a new, null pointer.
    pub const fn null() -> Self {
        Self(None)
    }

    /// Creates a new pointer.
    pub const fn new(ptr: u16) -> Self {
        Self(NonZero::new(ptr))
    }

    /// Returns whether or not the pointer is `null`.
    pub const fn is_null(&self) -> bool {
        self.get().is_none()
    }

    /// Returns `true` if the pointer is valid.
    pub const fn is_valid(&self) -> bool {
        !self.is_null()
    }

    /// Returns the contained pointer if it's valid.
    pub const fn get(&self) -> Option<u16> {
        match self.0 {
            Some(ptr) => Some(ptr.get()),
            None => None,
        }
    }

    /// Returns the pointers index in the inode table.
    pub const fn get_table_idx(&self) -> Option<u16> {
        match self.0 {
            Some(ptr) => Some(ptr.get() - 1), // sub 1 so inode 0 isn't excluded for being 'null'
            None => None,
        }
    }
}

impl InodeLookup {
    /// The number of bytes available in a name.
    const NAME_LEN: usize = 46;
}

impl BlockPtr {
    /// The maximum possible value for a pointer, due to limits in [`DualBlockPtr`].
    pub const MAX_VAL: u16 = 2u16.pow(12);

    /// Returns a new, null pointer.
    pub const fn null() -> Self {
        Self(None)
    }

    /// Creates a new pointer.
    /// Truncates the pointer to [`BlockPtr::MAX_VAL`] if it is greater.
    pub const fn new(ptr: u16) -> Self {
        if ptr < Self::MAX_VAL {
            Self(NonZero::new(ptr))
        } else {
            Self(NonZero::new(Self::MAX_VAL))
        }
    }

    /// Returns the contained pointer if it exists.
    pub const fn get(&self) -> Option<u16> {
        match self.0 {
            // so sad we can't just map it
            Some(n) => Some(n.get()),
            None => None,
        }
    }

    /// Returns the contained pointer if it exists or `0` if not.
    pub const fn get_nullable(&self) -> u16 {
        match self.get() {
            Some(val) => val,
            None => 0,
        }
    }

    /// Returns whether or not the pointer is `null`.
    pub const fn is_null(&self) -> bool {
        self.get().is_none()
    }

    /// Returns `true` if the pointer is valid.
    pub const fn is_valid(&self) -> bool {
        !self.is_null()
    }
}

impl Display for BlockPtr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.get() {
            Some(ptr) => write!(f, "block ptr {ptr}"),
            None => write!(f, "null block ptr"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that various types have the correct size.
    #[test]
    fn types_have_the_right_size() {
        assert_eq!(size_of::<INode>(), 128);
        assert_eq!(size_of::<INode>(), BLOCK_SIZE / 4);
        assert_eq!(size_of::<InodeLookup>(), 48);
        assert_eq!(size_of::<InodePtr>(), size_of::<u16>());
        assert_eq!(size_of::<BlockPtr>(), size_of::<u16>());
    }

    /// Performs some tests on [`BlockPtr`] nullability.
    #[test]
    #[rustfmt::skip]
    fn block_ptr_nullability() {
        assert!(  BlockPtr::null().is_null()                    );
        assert!(  !BlockPtr::null().is_valid()                  );
        assert!(  BlockPtr::new(0).is_null()                    );
        assert!(  BlockPtr::new(2312).is_valid()                );
        assert!(  BlockPtr::new(BlockPtr::MAX_VAL).is_valid()   );
        assert!(  BlockPtr::new(BlockPtr::MAX_VAL+6).is_valid() );
    }

    /// Tests the equality comparisons of [`BlockPtr`]. 
    #[test]
    #[rustfmt::skip]
    fn block_ptr_eq() {
        assert_eq!( BlockPtr::new(0), BlockPtr::null() );
        assert_ne!( BlockPtr::new(1), BlockPtr::null() );
        assert_eq!( BlockPtr::new(9), BlockPtr::new(9) );
        assert_eq!( BlockPtr::new(BlockPtr::MAX_VAL), BlockPtr::new(BlockPtr::MAX_VAL + 1) );
        assert_eq!( BlockPtr::new(BlockPtr::MAX_VAL), BlockPtr::new(BlockPtr::MAX_VAL + 257));
        assert_ne!( BlockPtr::new(BlockPtr::MAX_VAL), BlockPtr::new(BlockPtr::MAX_VAL - 1) );
    }

    /// Tests that dual block pointers are correctly encoded and decoded.
    #[test]
    fn dual_block_ptr_encoding() {
        for fst in 0..2u16.pow(12) {
            for snd in 0..2u16.pow(12) {
                let (fst, snd) = (BlockPtr::new(fst), BlockPtr::new(snd));
                let decoded = DualBlockPtr::encode([&fst, &snd]).decode();
                assert_eq!(fst, decoded[0]);
                assert_eq!(snd, decoded[1])
            }
        }
    }
}
