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
    kernel/src/vga/cursor.rs

    Handles the vga text mode cursor.
    Contained within the vga module
*/

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use super::buffers::{BUFFER_HEIGHT, BUFFER_WIDTH};
use crate::ports::{self, Port};

/// Allows printing to row 0 if set.
/// Used to prevent overwriting topbar.
pub static ALLOW_ROW_0: AtomicBool = AtomicBool::new(false);

/// The VGA's current cursor position.
static CURSOR: CursorPos = CursorPos {
      column: AtomicU8::new(0),
      row:    AtomicU8::new(0),
};

/// The position of the VGA cursor.
pub struct CursorPos {
      pub column: AtomicU8,
      pub row:    AtomicU8,
}

/// A direction which can cursor can be shifted using `shift_cursor`
pub enum CursorShift {
      Left,
      Right,
      Up,
      Down,
}

impl CursorPos {
      /// Returns the row and column fields of the static.
      pub fn row_col() -> (u8, u8) {
            let row = CURSOR.row.load(Ordering::Relaxed);
            let col = CURSOR.column.load(Ordering::Relaxed);
            (row, col)
      }

      /// Sets the row field in the static to `row`.
      pub fn set_row(row: u8) {
            CURSOR.row.store(row, Ordering::Relaxed);
            Self::clamp_row_col();
      }

      /// Sets the column field in the static to `col`.
      pub fn set_col(col: u8) {
            CURSOR.column.store(col, Ordering::Relaxed);
            Self::clamp_row_col();
      }

      /// Forces the row and column of the static to contain valid values.
      pub fn clamp_row_col() {
            let (row, col) = Self::row_col();

            // Clamp row
            let row = if ALLOW_ROW_0.load(Ordering::Relaxed) {
                  row.min(BUFFER_HEIGHT - 1)
            } else {
                  row.clamp(1, BUFFER_HEIGHT - 1)
            };
            CURSOR.row.store(row, Ordering::Relaxed);

            // Clamp column
            let col = col.min(BUFFER_WIDTH - 1);
            CURSOR.column.store(col, Ordering::Relaxed);
      }
}

/// Updates the visual position of the vga cursor on
/// the screen based off the `CURSOR` static.
pub fn update_visual_pos() {
      /// Index into the register for the low byte of the position.
      const REG_LOW_BYTE: u8 = 0x0E;

      /// Index into the register for the high byte of the position.
      const REG_HIGH_BYTE: u8 = 0x0F;

      CursorPos::clamp_row_col();
      let (row, col) = CursorPos::row_col();
      let pos = row as u16 * BUFFER_WIDTH as u16 + col as u16;

      // Safety: The cursor is forced into valid values
      // thanks to clamp_row_col
      unsafe {
            ports::writeb(Port::VGASelectorC, REG_LOW_BYTE);
            ports::writeb(Port::VGARegisterC, (pos >> 8) as u8);

            ports::writeb(Port::VGASelectorC, REG_HIGH_BYTE);
            ports::writeb(Port::VGARegisterC, pos as u8);
      }
}

/// Attempts to shift the cursor in one unit in `direction`.
pub fn shift_cursor(direction: CursorShift) {
      let (row, col) = CursorPos::row_col();

      match direction {
            CursorShift::Left => {
                  if col == 0 {
                        CursorPos::set_col(BUFFER_WIDTH - 1);
                  } else {
                        CursorPos::set_col(col - 1)
                  }
            }
            CursorShift::Right => {
                  if col < BUFFER_WIDTH - 1 {
                        CursorPos::set_col(col + 1);
                  } else {
                        CursorPos::set_col(0);
                  }
            }
            CursorShift::Up => {
                  if row == 1 {
                        CursorPos::set_row(BUFFER_HEIGHT - 1);
                  } else {
                        CursorPos::set_row(row - 1)
                  }
            }
            CursorShift::Down => {
                  if row < BUFFER_HEIGHT - 1 {
                        CursorPos::set_row(row + 1)
                  } else {
                        CursorPos::set_row(0)
                  }
            }
      };
}

#[cfg(test)]
mod tests {
      use super::*;

      /// Tests that `CursorPos::clamp_row_col()` clamps away invalid values.
      #[test_case]
      fn clamp_removes_invalid_values() {
            let (row, col) = CursorPos::row_col();
            let row0 = ALLOW_ROW_0.load(Ordering::Relaxed);

            ALLOW_ROW_0.store(false, Ordering::Relaxed);
            CursorPos::set_row(0);
            assert_eq!(1, CursorPos::row_col().0);

            ALLOW_ROW_0.store(true, Ordering::Relaxed);
            CursorPos::set_row(0);
            assert_eq!(0, CursorPos::row_col().0);

            CursorPos::set_row(u8::MAX);
            assert_eq!(BUFFER_HEIGHT - 1, CursorPos::row_col().0);

            CursorPos::set_col(u8::MAX);
            assert_eq!(BUFFER_WIDTH - 1, CursorPos::row_col().1);

            CursorPos::set_row(row);
            CursorPos::set_col(col);
            ALLOW_ROW_0.store(row0, Ordering::Relaxed);
      }
}
