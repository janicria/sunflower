use crate::{
    ports::{self, Port},
    vga::{self, CursorShift},
};
use core::{
    mem,
    sync::atomic::{AtomicBool, Ordering},
};

/// The current state of the keyboard.
static KEYBOARD: KeyboardState = KeyboardState::new();

#[derive(Clone, PartialEq)]
#[repr(u8)]
#[allow(unused)]
pub enum Key {
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

/// The action keys held or pressed on the keyboard.
#[derive(Default)]
struct KeyboardState {
    ctrl: AtomicBool,
    shift: AtomicBool,
    alt: AtomicBool,
    caps: AtomicBool,
}

impl KeyboardState {
    /// Creates a new `KeyboardState`.
    const fn new() -> Self {
        KeyboardState {
            ctrl: AtomicBool::new(false),
            shift: AtomicBool::new(false),
            alt: AtomicBool::new(false),
            caps: AtomicBool::new(false),
        }
    }

    /// Toggles the `caps` bool.
    fn flip_caps(&self) {
        self.caps.fetch_xor(true, Ordering::Relaxed);
    }
}

impl TryFrom<Key> for char {
    type Error = ();
    fn try_from(key: Key) -> Result<Self, Self::Error> {
        let shift = KEYBOARD.shift.load(Ordering::Relaxed);
        let caps = KEYBOARD.caps.load(Ordering::Relaxed);
        return match key {
            Key::One => default_or_shift('1', '!', shift),
            Key::Two => default_or_shift('2', '@', shift),
            Key::Three => default_or_shift('3', '#', shift),
            Key::Four => default_or_shift('4', '$', shift),
            Key::Five => default_or_shift('5', '%', shift),
            Key::Six => default_or_shift('6', '^', shift),
            Key::Seven => default_or_shift('7', '&', shift),
            Key::Eight => default_or_shift('8', '*', shift),
            Key::Nine => default_or_shift('9', '(', shift),
            Key::Zero => default_or_shift('0', ')', shift),

            Key::Minus => default_or_shift('-', '_', shift),
            Key::Equals => default_or_shift('=', '+', shift),
            Key::BracketLeft => default_or_shift('[', '{', shift),
            Key::BracketRight => default_or_shift(']', '}', shift),
            Key::Enter => Ok('\n'),
            Key::Space => Ok(' '),
            Key::Comma => default_or_shift(',', '<', shift),
            Key::Fullstop => default_or_shift('.', '>', shift),
            Key::Home => default_or_shift('`', '~', shift),
            Key::SemiColon => default_or_shift(';', ':', shift),
            Key::Quote => default_or_shift('\'', '\"', shift),
            Key::Backslash => default_or_shift('\\', '|', shift),
            Key::ForwardSlash => default_or_shift('/', '?', shift),

            Key::Q => default_or_shift('q', 'Q', shift || caps),
            Key::W => default_or_shift('w', 'W', shift || caps),
            Key::E => default_or_shift('e', 'E', shift || caps),
            Key::R => default_or_shift('r', 'R', shift || caps),
            Key::T => default_or_shift('t', 'T', shift || caps),
            Key::Y => default_or_shift('y', 'Y', shift || caps),
            Key::U => default_or_shift('u', 'U', shift || caps),
            Key::I => default_or_shift('i', 'I', shift || caps),
            Key::O => default_or_shift('o', 'O', shift || caps),
            Key::P => default_or_shift('p', 'P', shift || caps),
            Key::A => default_or_shift('a', 'A', shift || caps),
            Key::S => default_or_shift('s', 'S', shift || caps),
            Key::D => default_or_shift('d', 'D', shift || caps),
            Key::F => default_or_shift('f', 'F', shift || caps),
            Key::G => default_or_shift('g', 'G', shift || caps),
            Key::H => default_or_shift('h', 'H', shift || caps),
            Key::J => default_or_shift('j', 'J', shift || caps),
            Key::K => default_or_shift('k', 'K', shift || caps),
            Key::L => default_or_shift('l', 'L', shift || caps),
            Key::Z => default_or_shift('z', 'Z', shift || caps),
            Key::X => default_or_shift('x', 'X', shift || caps),
            Key::C => default_or_shift('c', 'C', shift || caps),
            Key::V => default_or_shift('v', 'V', shift || caps),
            Key::B => default_or_shift('b', 'B', shift || caps),
            Key::N => default_or_shift('n', 'N', shift || caps),
            Key::M => default_or_shift('m', 'M', shift || caps),

            _ => Err(()),
        };

        fn default_or_shift(default: char, shift: char, is_shift: bool) -> Result<char, ()> {
            Ok(if is_shift { shift } else { default })
        }
    }
}

/// Prints the scancode in set 1. Assumes US keyboard layout.
// Reference: https://wiki.osdev.org/PS/2_Keyboard#Scan_Code_Set_1
#[unsafe(no_mangle)]
extern "C" fn key_pressed_handler() {
    let scancode = unsafe { ports::readb(Port::PS2Data) };
    let key = unsafe { mem::transmute_copy::<u8, Key>(&scancode) };

    // My keyboard’s enter key is permanently stuck down…
    if cfg!(feature = "disable_enter") && key == Key::Enter {
        return;
    }

    match char::try_from(key) {
        Ok(c) => print!("{c}"),
        Err(_) => match scancode {
            0x2A | 0x36 => KEYBOARD.shift.store(true, Ordering::Relaxed), // shift pressed
            0xAA | 0xB6 => KEYBOARD.shift.store(false, Ordering::Relaxed), // shift released
            0x3A => KEYBOARD.flip_caps(),                                 // caps pressed
            0x1D => KEYBOARD.ctrl.store(true, Ordering::Relaxed),         // left ctrl released
            0x9D => KEYBOARD.ctrl.store(false, Ordering::Relaxed),        // left ctrl released
            0x38 => KEYBOARD.alt.store(true, Ordering::Relaxed),          // left alt pressed
            0xB8 => KEYBOARD.shift.store(false, Ordering::Relaxed),       // left alt released
            0x48 => vga::shift_cursor(CursorShift::Up),                   // arrow keys up
            0x4B => vga::shift_cursor(CursorShift::Left),                 // arrow keys left
            0x4D => vga::shift_cursor(CursorShift::Right),                // arrow keys right
            0x50 => vga::shift_cursor(CursorShift::Down),                 // arrow keys down
            0x0E => vga::delete_prev_char(),                              // backspace pressed
            _ => (),
        },
    };

    vga::update_vga_cursor();
}
