use crate::ports::{self, Port};
use core::fmt;
use spin::{lazy::Lazy, mutex::Mutex};

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
    LightBLue = 9,
    Lime = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

/// A character value supported by `VGA`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct VGAChar(u16);

impl VGAChar {
    /// Constructs a new color using `fg` as the text color and `bg` as the background color.
    const fn new(char: u8, fg: Color, bg: Color) -> VGAChar {
        VGAChar((char as u16) | (bg as u16) << 12 | (fg as u16) << 8)
    }
}

const BUFFER_WIDTH: usize = 80;
const BUFFER_HEIGHT: usize = 25;
const SPACE: VGAChar = VGAChar::new(0, Color::White, Color::Black);

/// The VGA text buffer.
#[repr(transparent)]
struct Buffer {
    chars: [[VGAChar; BUFFER_WIDTH]; BUFFER_HEIGHT],
}

/// A global `Writer` used by `print!` and `println` to write the VGA buffer.
pub static WRITER: Lazy<Mutex<Writer>> = Lazy::new(|| {
    Mutex::new(Writer {
        cursor_column: 0,
        cursor_row: 0,
        buffer: unsafe { &mut *(0xb8000 as *mut Buffer) },
    })
});

/// The writer used to write bytes to the `VGA`.
pub struct Writer {
    cursor_column: usize,
    cursor_row: usize,
    buffer: &'static mut Buffer,
}

/// See `Writer::shift_cursor`
pub enum CursorShift {
    Left,
    Right,
    Up,
    Down,
}

impl Writer {
    /// Writes `byte` to VGA as a character using `fg` as the text color and `bg` as the background color.
    fn write_char(&mut self, byte: u8, fg: Color, bg: Color) {
        match byte {
            b'\n' => self.newline(),
            byte => {
                // Allow text to wrap around screen
                if self.cursor_column >= BUFFER_WIDTH {
                    self.newline();
                }

                self.buffer.chars[self.cursor_row][self.cursor_column] = VGAChar::new(byte, fg, bg);
                self.cursor_column += 1;
            }
        }
    }

    /// Writes `s` to VGA as an sequence of bytes using `fg` as the text color and `bg` as the background color.
    fn write_str(&mut self, s: &str, fg: Color, bg: Color) {
        for byte in s.bytes() {
            self.write_char(byte, fg, bg);
        }
        self.update_cursor_pos();
    }

    /// Prints a newline
    fn newline(&mut self) {
        self.cursor_column = 0;

        // If we've reached the end move all rows up one and clear the last row
        if self.cursor_row == BUFFER_HEIGHT - 1 {
            for row in 1..BUFFER_HEIGHT {
                self.buffer.chars[row - 1] = self.buffer.chars[row]
            }

            let blank = VGAChar::new(b' ', Color::Black, Color::Black);
            for col in 0..BUFFER_WIDTH {
                self.buffer.chars[BUFFER_HEIGHT - 1][col] = blank
            }
        } else {
            // The cursor row will be forever stuck at BUFFER_HEIGHT once it's reached it
            self.cursor_row += 1;
        }

        self.update_cursor_pos();
    }

    /// Updates the cursor's position
    fn update_cursor_pos(&self) {
        let pos = self.cursor_row * BUFFER_WIDTH + self.cursor_column;
        unsafe {
            // tell vga we're going to be giving it the first byte of the pos
            ports::writeb(Port::VGAIndexRegister0x3D4, 0x0E);
            ports::writeb(Port::VgaCursorPos, (pos >> 8) as u8);

            // then that we're giving it the second byte
            ports::writeb(Port::VGAIndexRegister0x3D4, 0x0F);
            ports::writeb(Port::VgaCursorPos, (pos & 0xFF) as u8);
        }
    }

    /// Deletes the character to the left of the cursor.
    pub fn delete_previous(&mut self) {
        let (prev_col, prev_row) = if self.cursor_column == 0 {
            (BUFFER_WIDTH, self.cursor_row - 1)
        } else {
            (self.cursor_column - 1, self.cursor_row)
        };

        self.buffer.chars[prev_row][prev_col] = SPACE;
        self.shift_cursor(CursorShift::Left);
    }

    /// Attempts to shift the cursor in the direction of `direction`.
    pub fn shift_cursor(&mut self, direction: CursorShift) {
        match direction {
            CursorShift::Left => {
                if self.cursor_column == 0 {
                    self.cursor_column = BUFFER_WIDTH - 1;
                } else {
                    self.cursor_column -= 1
                }
            }
            CursorShift::Right => {
                if self.cursor_column < BUFFER_WIDTH - 1 {
                    self.cursor_column += 1
                } else {
                    self.cursor_column = 0
                }
            }
            CursorShift::Up => {
                if self.cursor_row == 0 {
                    self.cursor_row = BUFFER_HEIGHT - 1
                } else {
                    self.cursor_row -= 1
                }
            }
            CursorShift::Down => {
                if self.cursor_row < BUFFER_HEIGHT - 1 {
                    self.cursor_row += 1
                } else {
                    self.cursor_row = 0
                }
            }
        };

        self.update_cursor_pos();
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_str(s, Color::White, Color::Black);
        Ok(())
    }
}

#[macro_export]
macro_rules! print {
    ($($args:tt)+) => ($crate::vga::_print(format_args!($($args)+)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)+) => ($crate::print!("{}\n", format_args!($($arg)+)));
}

/// Used by `print!` and `println!` to write to the VGA buffer.
pub fn _print(args: fmt::Arguments) {
    use fmt::Write;
    write!(WRITER.lock(), "{args}").unwrap();
}

/// Prints `s` using `fg` as the text color and `bg` as the background color.
pub fn print_color(s: &str, fg: Color, bg: Color) {
    WRITER.lock().write_str(s, fg, bg);
}

/// Fills the buffer with spaces, allowing the cursor to blink anywhere.
pub(super) fn init() {
    {
        let buf = &mut WRITER.lock().buffer;
        buf.chars = [[SPACE; BUFFER_WIDTH]; BUFFER_HEIGHT];
    }
    // Print welcome message
    print!("Welcome to ");
    print_color("Sunflower!\n", Color::LightCyan, Color::Black);
    print_done("Filled VGA")
}

/// Prints the done messages used when first booting.
pub fn print_done(task: &str) {
    print!("[ ");
    print_color("DONE", Color::Lime, Color::Black);
    println!(" ] {task}");
}
