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
    libfs/src/header.rs

    The filesystem header
*/
use core::{cmp::Ordering, fmt::Display, mem};
use bitflags::bitflags;
use libutil::AsBytes;

/// The first block of the filesystem.
#[repr(C, packed)]
pub struct FilesystemHeader {
    /// The filesystem magic number, should be `0xFD SFK x86 x64` (0xFD53464B8664)
    pub magic: [u8; 6],

    /// When the filesystem was last updated
    pub release: FsRelease,

    /// The features available on the filesystem.
    pub features: FsFeatures,

    /// The name of the filesystem.
    pub name: [u8; Self::NAME_LEN],

    /// The size of the filesystem in blocks.
    pub size: u64,

    // reserved to reach a size of 512 bytes, or one block
    _reserved: [u8; Self::RESERVED_BYTES],
}

/// Represents when a filesystem was last updated in UTC
/// - Bits 0-9 - the day of the year (Jan 1st = 1)
/// - Bits 10-15 - the number of years since 2025
#[derive(PartialEq, Clone, Copy)]
#[repr(transparent)]
pub struct FsRelease(u16);

bitflags! {
    #[derive(Clone, Copy)]
    pub struct FsFeatures: u64 {
        /// The filesystem is connected to a floppy drive.
        const FLOPPY = 1;

        const _ = !0;
    }
}

impl FilesystemHeader {
    /// The length of the `name` field, in bytes.
    const NAME_LEN: usize = 16;

    /// The number of reserved bytes at the end of the header.
    const RESERVED_BYTES: usize = 472;

    /// Creates a new fsheader from the given fields.
    pub const fn new(
        name: [u8; Self::NAME_LEN],
        day: u16,
        year: u16,
        size: u64,
        feats: FsFeatures,
    ) -> FilesystemHeader {
        FilesystemHeader {
            magic: crate::MAGIC,
            release: FsRelease::new(day, year),
            features: feats,
            name,
            size,
            _reserved: [0; Self::RESERVED_BYTES],
        }
    }

    /// Converts an array of bytes into a header.
    pub fn from_raw(bytes: [u8; size_of::<FilesystemHeader>()]) -> Self {
        // Safety: All bit patterns of filesystem header are valid
        unsafe { mem::transmute::<[u8; size_of::<FilesystemHeader>()], FilesystemHeader>(bytes) }
    }

    /// Returns a copy of the header's features.
    pub fn features(&self) -> FsFeatures {
        self.features
    }

    /// Returns a copy of the header's release.
    pub fn release(&self) -> FsRelease {
        self.release
    }
}

// Safety: Fsheader is packed w/o uninit bytes & has no interior mutability.
unsafe impl AsBytes for FilesystemHeader {}

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

impl Display for FsFeatures {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that various types have the correct size.
    #[test]
    fn types_have_the_right_size() {
        assert_eq!(size_of::<FilesystemHeader>(), 512);
        assert_eq!(size_of::<FsRelease>(), 2);
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
