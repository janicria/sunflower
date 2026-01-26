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
    kernel/src/vga.rs

    The vga module handles writing to the vga text buffer.
    This file is responsible for initialising the VGA driver and drawing the topbar.

    Contains 3 submodules:
    * buffers.rs - Handles writing to and swapping between buffers
    * cursor.rs - Handles the vga text mode cursor
    * print.rs - Defines print macros
*/

#[cfg(test)]
use crate::tests::write_serial;
use crate::{startup::ExitCode, sysinfo::SystemInfo};
use buffers::RawBuffer;
use core::{convert::Infallible, sync::atomic::Ordering};
use cursor::{ALLOW_ROW_0, CursorPos};
use print::Corner;

pub mod buffers;
pub mod cursor;
#[macro_use]
pub mod print;

/// Connects the `BUFFER` static to the vga text buffer,
/// and fills it with spaces, allowing the cursor to blink anywhere.
///
/// # Safety
/// The buffer must not be used ANYWHERE.
pub unsafe fn init() -> ExitCode<Infallible> {
    let buf = &raw mut buffers::BUFFER;
    // Safety: The static isn't being used anywhere else and is being loaded with a valid buf.
    unsafe { *buf = &mut *(Corner::TopLeft as usize as *mut RawBuffer) }
    buffers::clear();

    if cfg!(test) {
        #[cfg(test)]
        write_serial("\nRunning startup tests...\n");
    } else {
        print!("\nHello, ");
        println!(fg = LightCyan, "Sunflower!\n");
    }

    ExitCode::Infallible
}

/// Draws the topbar, ran every second by [`crate::interrupts::kbd_poll_loop`].
pub fn draw_topbar() {
    // Print at the top left corner
    let (prev_row, prev_col) = CursorPos::row_col();
    ALLOW_ROW_0.store(true, Ordering::Relaxed);
    CursorPos::set_row(0);
    CursorPos::set_col(0);

    let sysinfo = SystemInfo::now();
    print!(
        fg = Black,
        bg = LightGrey,
        "                  | Sunflower {:#6} | Help: SysRq / PrntScr F7 | {:#14}",
        sysinfo.sfk_version,
        sysinfo.patch_quote
    );

    // Restore previous vga state
    ALLOW_ROW_0.store(false, Ordering::Relaxed);
    CursorPos::set_row(prev_row);
    CursorPos::set_col(prev_col);
}
