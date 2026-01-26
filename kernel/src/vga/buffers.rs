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
    kernel/src/vga/buffers.rs

    Handles writing to and swapping between buffers.
    Contained within the vga module
*/

use super::{
    cursor::{self, CursorPos},
    print::VGAChar,
};
use core::ptr;
use libutil::UnsafeFlag;

/// The width of the VGA text buf, in chars.
pub const BUFFER_WIDTH: u8 = 80;

/// The height of the VGA text buf, in chars.
pub const BUFFER_HEIGHT: u8 = 25;

pub type RawBuffer = [[VGAChar; BUFFER_WIDTH as usize]; BUFFER_HEIGHT as usize];

/// Allows yoinking the VGA text buffer for your nefarious purposes.
///
/// All other buffer operations will fail before this is dropped.
pub struct YoinkedBuffer(&'static mut RawBuffer);

impl YoinkedBuffer {
    /// Tries to return a mutable reference to the buffer.
    ///
    /// Fails if the buffer is being used somewhere else.
    pub fn try_yoink() -> Option<Self> {
        if !BUFFER_HELD.load() {
            // Safety: BUFFER_HELD is private to YoinkedBuffer, and the
            // check above ensures that there'll probably be only one copy of BUFFER
            unsafe {
                BUFFER_HELD.store(true);
                Some(Self(BUFFER))
            }
        } else {
            None
        }
    }

    /// Returns a mutable reference to the buffer.
    pub fn buffer(&mut self) -> &mut RawBuffer {
        self.0
    }

    /// Returns a new empty buffer.
    pub const fn empty_buffer() -> RawBuffer {
        [[VGAChar::SPACE; BUFFER_WIDTH as usize]; BUFFER_HEIGHT as usize]
    }
}

impl Drop for YoinkedBuffer {
    fn drop(&mut self) {
        // Safety: BUFFER_HELD is private to YoinkedBuffer
        unsafe {
            BUFFER_HELD.store(false);
        }
    }
}

/// The VGA text buffer.
///
/// # Safety
/// Do not directly access this static unless you're certain no other prints will happen.
/// Use [this](YoinkedBuffer) instead
pub static mut BUFFER: &mut RawBuffer = &mut YoinkedBuffer::empty_buffer();

/// If the buffer is currently being held.
/// # Flag
/// YoinkedBuffer will assume it has complete access to `BUFFER` when this static is cleared.
pub static BUFFER_HELD: UnsafeFlag = UnsafeFlag::new(false);

/// Fills the VGA text buffer with spaces and resets the cursor position.
pub fn clear() {
    CursorPos::set_col(0);
    CursorPos::set_row(1);
    cursor::update_visual_pos();

    // Clear the buffer
    if let Some(mut buf) = YoinkedBuffer::try_yoink() {
        *buf.buffer() = [[VGAChar::SPACE; BUFFER_WIDTH as usize]; BUFFER_HEIGHT as usize]
    }
}

/// Swaps between the two buffers if the current one isn't currently being used.
#[allow(clippy::redundant_pattern_matching)]
pub fn swap() {
    /// Where the unused buffer is stored.
    static mut ALT_BUF: RawBuffer = YoinkedBuffer::empty_buffer();

    /// Have to use a static since we don't want to store a 4kb buffer on the stack.
    /// This is also why we can't just use ptr::swap
    static mut TMP: RawBuffer = YoinkedBuffer::empty_buffer();

    if let Some(_) = YoinkedBuffer::try_yoink() {
        // Safety: We can safely write to BUFFER as it'll stay yoinked
        // until dropped, all of the statics are well aligned & valid
        // and since they're statics, they shouldn't overlap in any way
        unsafe {
            ptr::copy_nonoverlapping(&raw const ALT_BUF, &raw mut TMP, 1);
            ptr::copy_nonoverlapping(BUFFER, &raw mut ALT_BUF, 1);
            ptr::copy_nonoverlapping(&raw const TMP, BUFFER, 1);
        }
    }
}
