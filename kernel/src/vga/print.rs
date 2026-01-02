use super::{
    buffers::{BUFFER_HEIGHT, BUFFER_WIDTH, YoinkedBuffer},
    cursor::{self, CursorPos, CursorShift},
};
use core::{
    fmt::{self, Write},
    sync::atomic::Ordering,
};

#[cfg(test)]
use crate::tests::write_serial;

/// The color palette used by `VGAChar`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
#[repr(u16)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Purple = 5,
    Brown = 6,
    Grey = 7,
    LightGrey = 8,
    LightBlue = 9,
    Lime = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

/// A character value supported by the VGA's [text mode](https://en.wikipedia.org/wiki/VGA_text_mode).
/// It has the following bit layout:
///
/// - Bits 0-7 ~ Character
/// - Bits 8-11 ~ Foreground [`color`](Color)
/// - Bits 12-15 ~ Background [`color`](Color) (bit 15 is sometimes blink)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct VGAChar(u16);

impl VGAChar {
    /// The space character.
    pub const SPACE: VGAChar = VGAChar::new(0x20, Color::White, Color::Black);

    /// Constructs a new color using `fg` as the text color and `bg` as the background color.
    pub const fn new(char: u8, fg: Color, bg: Color) -> VGAChar {
        VGAChar((char as u16) | (bg as u16) << 12 | (fg as u16) << 8)
    }

    /// Returns a reference to `self` as an int.
    pub const fn as_raw(&self) -> u16 {
        self.0
    }

    /// Returns a mutable reference to `self` as an int.
    pub const fn as_raw_mut(&mut self) -> &mut u16 {
        &mut self.0
    }
}

/// Prints to the vga text buffer.
#[macro_export]
macro_rules! print {
    (fg = $fg:ident, bg = $bg:ident, $($args:tt)+) => ($crate::vga::print::_print(format_args!($($args)+), $crate::vga::print::Color::$fg, $crate::vga::print::Color::$bg));
    (fg = $fg:ident, $($args:tt)+) => ($crate::print!(fg = $fg, bg = Black, "{}", format_args!($($args)+)));
    (bg = $fg:ident, $($args:tt)+) => ($crate::print!(fg = White, bg = $fg, "{}", format_args!($($args)+)));
    ($($args:tt)+) => ($crate::print!(fg = White, bg = Black, "{}", format_args!($($args)+)));
}

/// Prints to the vga text buffer with a trailing newline.
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    (fg = $fg:ident, $($arg:tt)+) => ($crate::print!(fg = $fg, bg = Black, "{}\n", format_args!($($arg)+)));
    (bg = $bg:expr, $($arg:tt)+) => ($crate::print!(fg = White, bg = $bg, "{}\n", format_args!($($arg)+)));
    ($($arg:tt)+) => ($crate::print!("{}\n", format_args!($($arg)+)));
}

/// Prints to the vga text buffer if the `debug_info` feature is enabled.
#[macro_export]
macro_rules! dbg_info {
    ($($arg:tt)+) => {{
        #[cfg(feature = "debug_info")]
        $crate::println!(fg = LightGrey, "debug: {}", format_args!($($arg)+))
    }};
}

/// Prints to the vga text buffer if the `debug_info` feature is enabled.
#[macro_export]
macro_rules! warn {
    ($($arg:tt)+) => {
    #[cfg(feature = "debug_info")]
    {
        $crate::print!(fg = LightRed, "warning: ");
        $crate::println!(fg = LightGrey, $($arg)+)
    }};
}

/// The memory addresses to the four corners of the VGA text buffer.
#[derive(PartialEq, Clone, Copy)]
#[repr(usize)]
pub enum Corner {
    TopLeft = 0xb8000,
    TopRight = 0xb809e,
    // BottomLeft = 0xb8efe,
    // BottomRight = 0xb903e,
}

/// Used by `_print` to print.
/// Uses `fg` as the text color and `bg` as the background color.
struct VGAWriter {
    fg: Color,
    bg: Color,
}

impl Write for VGAWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            write_char(byte, self.fg, self.bg);
        }

        #[cfg(test)]
        write_serial(s);

        Ok(())
    }
}

/// Used by `print!` and `println!` to write to the VGA text buffer.
pub fn _print(args: fmt::Arguments, fg: Color, bg: Color) {
    let mut writer = VGAWriter { fg, bg };
    write!(writer, "{args}").unwrap()
}

/// Writes `byte` to VGA as a character using `fg` as the text color and `bg` as the background color.
pub fn write_char(byte: u8, fg: Color, bg: Color) {
    match byte {
        b'\n' => newline(),
        byte => {
            let (row, col) = CursorPos::row_col();
            let newline = col >= BUFFER_WIDTH - 1;

            // Print character
            if let Some(mut buf) = YoinkedBuffer::try_yoink() {
                buf.buffer()[row as usize][col as usize] = VGAChar::new(byte, fg, bg);
            }

            if newline {
                self::newline();
            } else {
                CursorPos::set_col(col + 1);
            }
        }
    }
}

/// Prints a newline.
fn newline() {
    if let Some(mut buf) = YoinkedBuffer::try_yoink() {
        let (row, _) = CursorPos::row_col();
        CursorPos::set_col(0);
        let buf = buf.buffer();

        // If we've reached the end, move all rows (except topbar) up one and clear the last row
        if row >= BUFFER_HEIGHT - 1 {
            let top_row = !cursor::ALLOW_ROW_0.load(Ordering::Relaxed) as usize;
            for row in top_row..BUFFER_HEIGHT as usize - 1 {
                buf[row] = buf[row + 1]
            }

            // Clear the last row
            for col in 0..BUFFER_WIDTH {
                buf[BUFFER_HEIGHT as usize - 1][col as usize] = VGAChar::SPACE
            }
        } else {
            CursorPos::set_row(row + 1);
        }
    }
}

/// Deletes the character to the left of the cursor.
/// Equivalent to a backspace.
pub fn delete_prev_char() {
    if let Some(mut buf) = YoinkedBuffer::try_yoink() {
        let (row, col) = CursorPos::row_col();

        if col == 0 {
            buf.buffer()[row as usize - 1][BUFFER_WIDTH as usize - 1] = VGAChar::SPACE;
            drop(buf);
            cursor::shift_cursor(CursorShift::Left);
            cursor::shift_cursor(CursorShift::Up);
        } else {
            buf.buffer()[row as usize][col as usize - 1] = VGAChar::SPACE;
            drop(buf);
            cursor::shift_cursor(CursorShift::Left);
        }
    }
}
