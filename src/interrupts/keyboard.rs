#[repr(u8)]
enum Key {
    Escape = 0x01,
    One = 0x02,
    Two = 0x03,
    Three = 0x04,
}

struct KeyboardState {
    ctrl: bool,
    shift: bool,
    alt: bool,
    caps: bool,
    _super: bool,
}

// Prints the scancode in scancode set 1. Assumes US keyboard layout
pub(super) fn print_key(scancode: u8) {
    // todo: finish
    print!("*")
}
