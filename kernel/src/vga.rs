#[cfg(test)]
use crate::tests::write_serial;
use crate::{interrupts, startup::ExitCode, sysinfo::SystemInfo};
use buffers::RawBuffer;
use core::{convert::Infallible, sync::atomic::Ordering};
use cursor::{ALLOW_ROW_0, CursorPos};
use print::Corner;

/// Handles writing to and swapping between buffers.
pub mod buffers;

/// Handles the vga cursor's print & visual positions.
pub mod cursor;

/// Exports print macros & allows printing characters.
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

/// Draws the topbar with `title` as it's title.
/// Title must be exactly 9 bytes long.
pub fn draw_topbar(title: &'static str) {
    interrupts::cli();
    let len = title.len();

    // Force title to be nine bytes
    if len != 9 {
        warn!(
            "attempted setting topbar title with an invalid len ({len}), it will be truncated or discarded to preserve formatting!"
        );
    }
    let title = title.split_at_checked(9).unwrap_or(("Bad Title", "")).0;

    // Print at the top left corner
    let (prev_row, prev_col) = CursorPos::row_col();
    ALLOW_ROW_0.store(true, Ordering::Relaxed);
    CursorPos::set_row(0);
    CursorPos::set_col(0);

    // Do the printing
    let sysinfo = SystemInfo::now();
    print!(
        fg = Black,
        bg = LightGrey,
        " {} on {} | {title} | Help: SysRq / PrntScr F7 | {}",
        sysinfo.sfk_version_short,
        sysinfo.cpu_vendor,
        sysinfo.patch_quote
    );

    // Restore previous vga state
    ALLOW_ROW_0.store(false, Ordering::Relaxed);
    CursorPos::set_row(prev_row);
    CursorPos::set_col(prev_col);
    interrupts::sti();
}
