use crate::{
    interrupts,
    ports::{self, Port},
    sysinfo::SystemInfo,
    wrappers::UnsafeFlag,
};
use core::{
    convert::Infallible,
    fmt::{self, Write},
    ptr,
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
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
    /// The space character.
    const SPACE: VGAChar = VGAChar::new(0x20, Color::White, Color::Black);

    /// A box character for top left corners.
    pub const TOPLEFT_CORNER: u8 = 0xC9;

    /// A box character for bottom left corners.
    pub const BOTTOMLEFT_CORNER: u8 = 0xC8;

    /// A box character for top right corners.
    pub const TOPRIGHT_CORNER: u8 = 0xBB;

    /// A box character for bottom right corners.
    pub const BOTTOMRIGHT_CORNER: VGAChar = VGAChar::new(0xBC, Color::Grey, Color::Black);

    /// A box character for vertical borders.
    pub const VERTICAL_BORDER: VGAChar = VGAChar::new(0xBA, Color::White, Color::Black);

    /// A box character for horizontal borders.
    pub const HORIZONTAL_BORDER: VGAChar = VGAChar::new(0xCD, Color::White, Color::Black);

    /// Constructs a new color using `fg` as the text color and `bg` as the background color.
    pub const fn new(char: u8, fg: Color, bg: Color) -> VGAChar {
        VGAChar((char as u16) | (bg as u16) << 12 | (fg as u16) << 8)
    }
}

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

/// Allows printing to row 0 if set.
/// Used to prevent overwriting topbar.
static ALLOW_ROW_0: AtomicBool = AtomicBool::new(false);

/// The VGA's current cursor info.
///
/// Access this static via the `CursorPos` associated functions,
/// though nothing bad will happen if you access this directly.
static CURSOR: CursorPos = CursorPos {
    column: AtomicU8::new(0),
    row: AtomicU8::new(0),
};

/// Stores information about the VGA cursor.
pub struct CursorPos {
    pub column: AtomicU8,
    pub row: AtomicU8,
}

/// A direction which can cursor can be shifted using `shift_cursor`
pub enum CursorShift {
    Left,
    Right,
    Up,
    Down,
}

/// Prints to the vga text buffer.
#[macro_export]
macro_rules! print {
    (fg = $fg:ident, bg = $bg:ident, $($args:tt)+) => ($crate::vga::_print(format_args!($($args)+), $crate::vga::Color::$fg, $crate::vga::Color::$bg));
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

impl CursorPos {
    /// Returns the row and column fields of the static.
    fn row_col() -> (u8, u8) {
        let row = CURSOR.row.load(Ordering::Relaxed);
        let col = CURSOR.column.load(Ordering::Relaxed);
        (row, col)
    }

    /// Sets the row field in the static to `row`.
    pub fn set_row(row: u8) {
        CURSOR.row.store(row, Ordering::Relaxed);
        Self::clamp_row_col();
    }

    /// Sets th column field in the static to `col`.
    pub fn set_col(col: u8) {
        CURSOR.column.store(col, Ordering::Relaxed);
        Self::clamp_row_col();
    }

    /// Forces the row and column of the static to contain valid values.
    fn clamp_row_col() {
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

/// The memory addresses to the four corners of the VGA text buffer.
#[derive(PartialEq, Clone, Copy)]
#[repr(usize)]
pub enum Corner {
    TopLeft = 0xb8000,
    TopRight = 0xb809e,
    BottomLeft = 0xb8efe,
    BottomRight = 0xb903e,
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

            // Allow text to wrap around screen
            if newline {
                self::newline();
            }

            // Print character
            if let Some(mut buf) = YoinkedBuffer::try_yoink() {
                buf.buffer()[row as usize][col as usize] = VGAChar::new(byte, fg, bg);

                // Increase column if not newline
                if !newline {
                    CursorPos::set_col(col + 1);
                }
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

        // If we've reached the end move all rows up one and clear the last row
        if row >= BUFFER_HEIGHT - 1 {
            for row in 1..BUFFER_HEIGHT {
                buf[row as usize - 1] = buf[row as usize]
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

/// Updates the position of the vga cursor based on the `CURSOR` static.
pub fn update_vga_cursor() {
    CursorPos::clamp_row_col();
    let (row, col) = CursorPos::row_col();
    let pos = row as u16 * BUFFER_WIDTH as u16 + col as u16;

    // Safety: The cursor is forced into valid values in the line above
    unsafe {
        // tell vga we're going to be giving it the first byte of the pos
        ports::writeb(Port::VGAIndexRegister0x3D4, 0x0E);
        ports::writeb(Port::VgaCursorPos, (pos >> 8) as u8);

        // then that we're giving it the second byte
        ports::writeb(Port::VGAIndexRegister0x3D4, 0x0F);
        ports::writeb(Port::VgaCursorPos, pos as u8);
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
            shift_cursor(CursorShift::Left);
            shift_cursor(CursorShift::Up);
        } else {
            buf.buffer()[row as usize][col as usize - 1] = VGAChar::SPACE;
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

/// Fills the VGA text buffer with spaces and resets the cursor position.
pub fn clear() {
    CursorPos::set_col(0);
    CursorPos::set_row(1);
    update_vga_cursor();

    // Clear the buffer
    if let Some(mut buf) = YoinkedBuffer::try_yoink() {
        *buf.buffer() = [[VGAChar::SPACE; BUFFER_WIDTH as usize]; BUFFER_HEIGHT as usize]
    }
}

/// Swaps between the two buffers if the current one isn't currently being used.
pub fn swap_buffers() {
    /// Where the unused buffer is stored.
    static mut ALT_BUF: RawBuffer = YoinkedBuffer::empty_buffer();

    /// Have to use a static since we don't want to store a 4kb buffer on the stack.
    /// This is also why we can't just use ptr::swap
    static mut TMP: RawBuffer = YoinkedBuffer::empty_buffer();

    if YoinkedBuffer::try_yoink().is_some() {
        // Safety: We know we can write to BUFFER (see check above)
        // and both BUFFER, ALT_BUF & TMP are well aligned & valid
        unsafe {
            ptr::write_volatile(&raw mut TMP, ALT_BUF);
            ptr::write_volatile(&raw mut ALT_BUF, *BUFFER);
            ptr::write_volatile(BUFFER, TMP);
        }
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
    }

    // Print welcome message
    clear();
    print!("\nHello, ");
    println!(fg = LightCyan, "Sunflower!\n");
    Ok(())
}

/// Draws the topbar with `title` as it's title.
/// Title must be exactly 9 characters long.
pub fn draw_topbar(title: &'static str) {
    // Print at the top left corner
    interrupts::cli();
    let (prev_row, prev_col) = CursorPos::row_col();
    ALLOW_ROW_0.store(true, Ordering::Relaxed);
    CursorPos::set_row(0);
    CursorPos::set_col(0);

    // Do the printing
    let sysinfo = SystemInfo::now();
    print!(
        fg = Black,
        bg = Cyan,
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
