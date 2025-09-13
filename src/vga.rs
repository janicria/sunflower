use crate::ports::{self, Port};
use core::{
    convert::Infallible,
    fmt::{self, Write},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

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

/// A character value supported by `VGA`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct VGAChar(u16);

impl VGAChar {
    /// Constructs a new color using `fg` as the text color and `bg` as the background color.
    pub const fn new(char: u8, fg: Color, bg: Color) -> VGAChar {
        VGAChar((char as u16) | (bg as u16) << 12 | (fg as u16) << 8)
    }
}

pub const BUFFER_WIDTH: usize = 80;
pub const BUFFER_HEIGHT: usize = 25;
const SPACE: VGAChar = VGAChar::new(0, Color::White, Color::Black);

pub type RawBuffer = [[VGAChar; BUFFER_WIDTH]; BUFFER_HEIGHT];

/// Allows yoinking the VGA text buffer for your nefarious purposes.
///
/// All other buffer operations will fail before this is dropped.
pub struct YoinkedBuffer(&'static mut RawBuffer);

impl YoinkedBuffer {
    /// Tries to return a mutable reference to the buffer.
    ///
    /// Fails if the buffer is being used somewhere else.
    fn try_yoink() -> Option<Self> {
        if !BUFFER_HELD.load(Ordering::Relaxed) {
            BUFFER_HELD.store(true, Ordering::Relaxed);
            // SAFETY: The check above ensures that there will probably only be one copy of BUFFER
            unsafe { Some(Self(BUFFER)) }
        } else {
            None
        }
    }

    /// Returns a mutable reference to the buffer.
    fn buffer(&mut self) -> &mut RawBuffer {
        self.0
    }

    /// Returns a new empty buffer.
    pub const fn empty_buffer() -> RawBuffer {
        [[VGAChar::new(0, Color::Black, Color::Black); BUFFER_WIDTH]; BUFFER_HEIGHT]
    }
}

impl Drop for YoinkedBuffer {
    fn drop(&mut self) {
        BUFFER_HELD.store(false, Ordering::Relaxed);
    }
}

/// The VGA text buffer.
///
/// # Safety
/// Do not directly access this static unless you're certain no other prints will happen.
/// Use [this](YoinkedBuffer) instead
pub static mut BUFFER: &mut RawBuffer = &mut YoinkedBuffer::empty_buffer();

/// If the buffer is currently being held.
static BUFFER_HELD: AtomicBool = AtomicBool::new(false);

/// The VGA's current cursor info.
pub static CURSOR: CursorPos = CursorPos {
    column: AtomicUsize::new(0),
    row: AtomicUsize::new(0),
};

/// Stores information about the VGA cursor.
pub struct CursorPos {
    pub column: AtomicUsize,
    pub row: AtomicUsize,
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
    fn row_col() -> (usize, usize) {
        let row = CURSOR.row.load(Ordering::Relaxed);
        let col = CURSOR.column.load(Ordering::Relaxed);
        (row, col)
    }
}

/// The memory addresses to the four corners of the VGA text buffer.
#[derive(PartialEq, Clone, Copy)]
#[repr(usize)]
pub enum Corner {
    TopLeft = 0xb8000,
    TopRight = 0xb809e,
    BottomLeft = 0xb8efe,
    BottomRight = 0xb903e,
}

/// Prints to the vga text buffer.
#[macro_export]
macro_rules! print {
    ($($args:tt)+) => ($crate::vga::_print(format_args!($($args)+)));
}

/// Prints to the vga text buffer with a trailing newline.
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)+) => ($crate::print!("{}\n", format_args!($($arg)+)));
}

/// Used by `_print` to write call `print_color`.
struct VGAWriter;

impl Write for VGAWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        print_color(s, Color::White);
        Ok(())
    }
}

/// Used by `print!` and `println!` to write to the VGA text buffer.
pub fn _print(args: fmt::Arguments) {
    write!(VGAWriter, "{args}").unwrap()
}

/// Prints `s` using `fg` as the text color and black as the background color.
pub fn print_color(s: &str, fg: Color) {
    for byte in s.bytes() {
        write_char(byte, fg, Color::Black);
    }
}

/// Writes `byte` to VGA as a character using `fg` as the text color and `bg` as the background color.
pub fn write_char(byte: u8, fg: Color, bg: Color) {
    match byte {
        b'\n' => newline(),
        byte => {
            let (row, col) = CursorPos::row_col();
            let newline = col >= BUFFER_WIDTH - 1;

            // Allow text to wrap around screen
            if col >= BUFFER_WIDTH - 1 {
                self::newline();
            }

            // Print character
            if let Some(mut buf) = YoinkedBuffer::try_yoink() {
                buf.buffer()[row][col] = VGAChar::new(byte, fg, bg);

                // Newline sets column to 0, so increasing it here would make a random gap appear
                if !newline {
                    CURSOR.column.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}

/// Prints a newline.
fn newline() {
    if let Some(mut buf) = YoinkedBuffer::try_yoink() {
        CURSOR.column.store(0, Ordering::Relaxed);
        let buf = buf.buffer();

        // If we've reached the end move all rows up one and clear the last row
        if CURSOR.row.load(Ordering::Relaxed) >= BUFFER_HEIGHT - 1 {
            for row in 1..BUFFER_HEIGHT {
                buf[row - 1] = buf[row]
            }

            // clear last row
            for col in 0..BUFFER_WIDTH {
                buf[BUFFER_HEIGHT - 1][col] = SPACE
            }
        } else {
            CURSOR.row.fetch_add(1, Ordering::Acquire);
        }
    }
}

/// Updates the position of the vga cursor based on the `CURSOR` static.
pub fn update_vga_cursor() {
    let (row, col) = CursorPos::row_col();
    let pos = row * BUFFER_WIDTH + col;

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
/// Equivalent to a backspace.
pub fn delete_prev_char() {
    if let Some(mut buf) = YoinkedBuffer::try_yoink() {
        let (row, col) = CursorPos::row_col();

        if col == 0 {
            buf.buffer()[row - 1][BUFFER_WIDTH - 1] = SPACE;
            drop(buf);
            shift_cursor(CursorShift::Left);
            shift_cursor(CursorShift::Up);
        } else {
            buf.buffer()[row][col - 1] = SPACE;
            drop(buf);
            shift_cursor(CursorShift::Left);
        }
    }
}

/// Attempts to shift the cursor in one unit in `direction`.
pub fn shift_cursor(direction: CursorShift) {
    let (row, col) = CursorPos::row_col();

    match direction {
        CursorShift::Left => {
            if col == 0 {
                CURSOR.column.store(BUFFER_WIDTH - 1, Ordering::Relaxed)
            } else {
                CURSOR.column.fetch_sub(1, Ordering::Relaxed);
            }
        }
        CursorShift::Right => {
            if col < BUFFER_WIDTH - 1 {
                CURSOR.column.fetch_add(1, Ordering::Relaxed);
            } else {
                CURSOR.column.store(0, Ordering::Relaxed)
            }
        }
        CursorShift::Up => {
            if row == 0 {
                CURSOR.row.store(BUFFER_HEIGHT - 1, Ordering::Relaxed);
            } else {
                CURSOR.row.fetch_sub(1, Ordering::Relaxed);
            }
        }
        CursorShift::Down => {
            if row < BUFFER_HEIGHT - 1 {
                CURSOR.row.fetch_add(1, Ordering::Relaxed);
            } else {
                CURSOR.row.store(0, Ordering::Relaxed);
            }
        }
    };

    update_vga_cursor();
}

/// Fills the VGA text buffer with spaces and resets the cursor position.
pub fn clear() {
    unsafe {
        *BUFFER = [[SPACE; BUFFER_WIDTH]; BUFFER_HEIGHT];
        CURSOR.column.store(0, Ordering::Relaxed);
        CURSOR.row.store(0, Ordering::Relaxed);
        update_vga_cursor();
    }
}

/// Connects the `BUFFER` static to the vga text buffer.
///
/// Fills it with spaces, allowing the vga cursor to blink anywhere.
///
/// Finally, prints the welcome message.
pub fn init() -> Result<(), Infallible> {
    unsafe {
        let buf = &raw mut BUFFER;
        *buf = &mut *(Corner::TopLeft as usize as *mut RawBuffer);
        clear();
    }

    // Print welcome message
    print!("Hello, ");
    print_color("Sunflower!\n\n", Color::LightCyan);
    Ok(())
}
