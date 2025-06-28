use core::mem;
use spin::Mutex;

use crate::vga::{self, CursorShift};

#[allow(unused)]
#[repr(u8)]
enum Key {
    Escape = 0x01,
    One,
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,
    Zero,

    Minus,
    Equals,

    Q = 0x10,
    W,
    E,
    R,
    T,
    Y,
    U,
    I,
    O,
    P,

    BracketLeft,
    BracketRight,
    Enter = 0x1C,

    A = 0x1E,
    S,
    D,
    F,
    G,
    H,
    J,
    K,
    L,

    SemiColon,
    Quote,
    Home = 0x29,
    Backslash = 0x2B,

    Z,
    X,
    C,
    V,
    B,
    N,
    M,

    Comma,
    Fullstop,
    ForwardSlash = 0x35,

    Space = 0x39,
    F1 = 0x3B,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    NumLock,
    ScrollLock = 0x46,
    F11 = 0xD7,
    F12 = 0xD8,

    Unknown,
}

static KEYBOARD: Mutex<KeyboardState> =
    Mutex::new(unsafe { mem::transmute::<u32, KeyboardState>(0) });

#[derive(Default)]
struct KeyboardState {
    ctrl: bool,
    shift: bool,
    alt: bool,
    caps: bool,
    //_super: bool,
}

impl KeyboardState {
    fn toggle_caps(&mut self) {
        self.caps = !self.caps
    }
}

macro_rules! key_as_char {
    ($normal: expr, $shift: expr, $is_shift: expr) => {
        Ok(if $is_shift { $shift } else { $normal })
    };
}

impl TryFrom<Key> for char {
    type Error = ();
    fn try_from(key: Key) -> Result<Self, Self::Error> {
        let lock = KEYBOARD.lock();
        let shift = lock.shift;
        let caps = lock.caps;
        match key {
            Key::One => key_as_char!('1', '!', shift),
            Key::Two => key_as_char!('2', '@', shift),
            Key::Three => key_as_char!('3', '#', shift),
            Key::Four => key_as_char!('4', '$', shift),
            Key::Five => key_as_char!('5', '%', shift),
            Key::Six => key_as_char!('6', '^', shift),
            Key::Seven => key_as_char!('7', '&', shift),
            Key::Eight => key_as_char!('8', '*', shift),
            Key::Nine => key_as_char!('9', '(', shift),
            Key::Zero => key_as_char!('0', ')', shift),

            Key::Minus => key_as_char!('-', '_', shift),
            Key::Equals => key_as_char!('=', '+', shift),
            Key::BracketLeft => key_as_char!('[', '{', shift),
            Key::BracketRight => key_as_char!(']', '}', shift),
            Key::Enter => Ok('\n'),
            Key::Space => Ok(' '),
            Key::Comma => key_as_char!(',', '<', shift),
            Key::Fullstop => key_as_char!('.', '>', shift),
            Key::Home => key_as_char!('`', '~', shift),
            Key::SemiColon => key_as_char!(';', ':', shift),
            Key::Quote => key_as_char!('\'', '\"', shift),
            Key::Backslash => key_as_char!('\\', '|', shift),
            Key::ForwardSlash => key_as_char!('/', '?', shift),

            Key::Q => key_as_char!('q', 'Q', shift || caps),
            Key::W => key_as_char!('w', 'W', shift || caps),
            Key::E => key_as_char!('e', 'E', shift || caps),
            Key::R => key_as_char!('r', 'R', shift || caps),
            Key::T => key_as_char!('t', 'T', shift || caps),
            Key::Y => key_as_char!('y', 'Y', shift || caps),
            Key::U => key_as_char!('u', 'U', shift || caps),
            Key::I => key_as_char!('i', 'I', shift || caps),
            Key::O => key_as_char!('o', 'O', shift || caps),
            Key::P => key_as_char!('p', 'P', shift || caps),
            Key::A => key_as_char!('a', 'A', shift || caps),
            Key::S => key_as_char!('s', 'S', shift || caps),
            Key::D => key_as_char!('d', 'D', shift || caps),
            Key::F => key_as_char!('f', 'F', shift || caps),
            Key::G => key_as_char!('g', 'G', shift || caps),
            Key::H => key_as_char!('h', 'H', shift || caps),
            Key::J => key_as_char!('j', 'J', shift || caps),
            Key::K => key_as_char!('k', 'K', shift || caps),
            Key::L => key_as_char!('l', 'L', shift || caps),
            Key::Z => key_as_char!('z', 'Z', shift || caps),
            Key::X => key_as_char!('x', 'X', shift || caps),
            Key::C => key_as_char!('c', 'C', shift || caps),
            Key::V => key_as_char!('v', 'V', shift || caps),
            Key::B => key_as_char!('b', 'B', shift || caps),
            Key::N => key_as_char!('n', 'N', shift || caps),
            Key::M => key_as_char!('m', 'M', shift || caps),

            _ => Err(()),
        }
    }
}

// Prints the scancode in scancode set 1. Assumes US keyboard layout
pub(super) fn print_key(scancode: u8) {
    let key: Key = unsafe { mem::transmute(scancode) };
    match char::try_from(key) {
        Ok(c) => print!("{c}"),
        Err(_) => match scancode {
            0x2A | 0x36 => KEYBOARD.lock().shift = true,   // shift pressed
            0xAA | 0xB6 => KEYBOARD.lock().shift = false,  // shift released
            0x3A => KEYBOARD.lock().toggle_caps(),         // caps pressed
            0x1D => KEYBOARD.lock().ctrl = true,           // left ctrl released
            0x38 => KEYBOARD.lock().alt = true,            // left alt pressed
            0xB8 => KEYBOARD.lock().alt = false,           // left alt released
            0x48 => vga::WRITER.lock().shift_cursor(CursorShift::Up),    // arrow keys up
            0x4B => vga::WRITER.lock().shift_cursor(CursorShift::Left),  // arrow keys left
            0x4D => vga::WRITER.lock().shift_cursor(CursorShift::Right), // arrow keys right
            0x50 => vga::WRITER.lock().shift_cursor(CursorShift::Down),  // arrow keys down
            0x0E => vga::WRITER.lock().delete_previous(),  // backspace pressed 
            _ => (),
        },
    }
}
