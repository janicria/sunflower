use crate::{
    ports::{self, Port},
    startup,
    vga::{self, CursorShift},
};
use core::{
    fmt::Display,
    sync::atomic::{AtomicU8, Ordering},
};
use pc_keyboard::{
    DecodedKey, HandleControl, KeyCode, KeyState, Keyboard, Modifiers, ScancodeSet2,
    layouts::Us104Key,
};
use ps2::{
    Controller,
    error::KeyboardError,
    flags::{ControllerConfigFlags, KeyboardLedFlags},
};

/// Circular scancode buffer. Each AtomicU8 represents a scancode.
/// The genius idea of this buffer was taken from
/// [`this video`](https://www.youtube.com/watch?v=dL0GO9SeBh0&list=PLUZozxlhse-NUto5JeJ0EDXEUFloWBdA).
static KBD_BUF: [AtomicU8; 256] = [const { AtomicU8::new(0) }; 256];

/// Index into the last handled scancode in the keyboard buffer.
static KBD_RPTR: AtomicU8 = AtomicU8::new(0);

/// Index into the last added scancode to the keyboard buffer.
static KBD_WPTR: AtomicU8 = AtomicU8::new(0);

/// Whether shift is being held or not. Used as pc-keyboard's shift check is dodgy.
///
/// - Bit 0 - Left shift
/// - Bit 1 - Right shift
static SHIFT: AtomicU8 = AtomicU8::new(0);

/// Enables external interrupts, disables mouse, runs some tests, 
/// sets config, then sets the scancode and numlock LEDs.
pub fn init() -> Result<(), KbdInitError> {
    super::sti();

    // Safety: kbd_handler returns immediately if !SYS_INIT, so this is the only use of ports 0x60 & 0x64.
    let mut controller = unsafe { Controller::new() };

    // Disable devices
    KbdInitError::map("Disable keyboard", controller.disable_keyboard())?;
    KbdInitError::map("Disable mouse", controller.disable_mouse())?;

    // It doesn't matter if it's an err since we're just flushing the buffer
    _ = controller.read_data();

    // Tests
    KbdInitError::map("Controller test", controller.test_controller())?;
    KbdInitError::map("Keyboard test", controller.test_keyboard())?;

    // Config
    let mut cfg = ControllerConfigFlags::all();
    cfg.set(ControllerConfigFlags::DISABLE_KEYBOARD, false); // enable kbd
    cfg.set(ControllerConfigFlags::ENABLE_MOUSE_INTERRUPT, false);
    cfg.set(ControllerConfigFlags::ENABLE_TRANSLATE, false); // so scancode set 2 is actually scancode set 2
    KbdInitError::map("Set config", controller.write_config(cfg))?;

    // Re-enable keyboard
    let mut kbd = controller.keyboard();
    KbdInitError::map("Keyboard echo", kbd.echo())?;

    // Scancode set 2 & Num Lock LEDs
    KbdInitError::map("Set scancode", kbd.set_scancode_set(2))?;
    KbdInitError::map("Set LEDS", kbd.set_leds(KeyboardLedFlags::NUM_LOCK))?;
    KbdInitError::map("Reset keyboard", kbd.reset_and_self_test())?;

    Ok(())
}

/// Error returned from `init`.
pub struct KbdInitError {
    cmd: &'static str,
    err: KeyboardError,
}

impl KbdInitError {
    /// Maps a `Result<T, E>` to a `Result<(), Self>`
    fn map<T, E>(cmd: &'static str, err: Result<T, E>) -> Result<(), Self>
    where
        E: Into<KeyboardError>,
    {
        match err {
            Err(err) => Err(KbdInitError {
                cmd,
                err: err.into(),
            }),
            Ok(_) => Ok(()),
        }
    }
}

impl Display for KbdInitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} -  {:?}", self.cmd, self.err)
    }
}

/// Adds the last response from the keyboard to the keyboard buffer.
/// # Safety
/// Reads from port 0x60 for it's response.
#[unsafe(no_mangle)]
unsafe fn kbd_handler() {
    /// The last value read from port 0x60.
    static PREV_RESPONSE: AtomicU8 = AtomicU8::new(0);

    if !startup::init() {
        return;
    }

    // Prevent another IRQ handler from adding to the buffer at the same time.
    super::cli();

    let ptr = KBD_WPTR.load(Ordering::Relaxed) as usize;
    let scancode = unsafe { ports::readb(Port::PS2Data) };

    // Save the scancode to the buffer
    KBD_WPTR.fetch_add(1, Ordering::Relaxed);
    KBD_BUF[ptr].store(scancode, Ordering::Relaxed);
    PREV_RESPONSE.store(scancode, Ordering::Relaxed);

    super::sti();
}

/// Polls the keyboard buffer for any new keys pressed.
#[allow(clippy::deref_addrof)]
pub fn poll_keyboard() {
    /// The current state of the keyboard
    static mut KBD: Keyboard<Us104Key, ScancodeSet2> =
        Keyboard::new(ScancodeSet2::new(), Us104Key, HandleControl::Ignore);

    // Left and right shift scancodes in set 2.
    static LSHIFT_SCANCODE: u8 = 0x12;
    static RSHIFT_SCANCODE: u8 = 0x59;

    let read_ptr = KBD_RPTR.load(Ordering::Relaxed);
    let write_ptr = KBD_WPTR.load(Ordering::Relaxed);

    // Safety: This is the only time keyboard is mutated
    let kbd = unsafe { &mut *&raw mut KBD };

    // Return if we've reached the end of the buffer
    if read_ptr >= write_ptr {
        KBD_RPTR.store(write_ptr, Ordering::Relaxed);
        return kbd.clear();
    }

    let scancode = KBD_BUF[read_ptr as usize].load(Ordering::Relaxed);
    KBD_RPTR.fetch_add(1, Ordering::Relaxed);

    // Handle shift
    if scancode == LSHIFT_SCANCODE {
        // Flip lshift bit
        SHIFT.fetch_xor(1 << 0, Ordering::Relaxed);
    } else if scancode == RSHIFT_SCANCODE {
        // Flip rshift bit
        SHIFT.fetch_xor(1 << 1, Ordering::Relaxed);
    }

    // If a key was pressed
    if let Ok(event) = kbd.add_byte(scancode)
        && let Some(event) = event
        && event.state == KeyState::Down
        && let Some(key) = kbd.process_keyevent(event)
    {
        match key {
            DecodedKey::RawKey(key) => handle_special_key(key),
            DecodedKey::Unicode(key) => print_key(key, kbd.get_modifiers()),
        }
    }
}

/// Handles when a "special" non-printable key is pressed.
fn handle_special_key(key: KeyCode) {
    match key {
        KeyCode::Return | KeyCode::NumpadEnter => print!("\n"),
        KeyCode::ArrowLeft => vga::shift_cursor(CursorShift::Left),
        KeyCode::ArrowRight => vga::shift_cursor(CursorShift::Right),
        KeyCode::ArrowUp => vga::shift_cursor(CursorShift::Up),
        KeyCode::ArrowDown => vga::shift_cursor(CursorShift::Down),
        KeyCode::Delete | KeyCode::Backspace => vga::delete_prev_char(),
        _ => (),
    }
}

/// Prints `key`.
fn print_key(mut key: char, kbd: &Modifiers) {
    /// Mapping of how to translate keys when shift is held.
    static SHIFT_KEYS: [(char, char); 21] = [
        ('1', '!'),
        ('2', '@'),
        ('3', '#'),
        ('4', '$'),
        ('5', '%'),
        ('6', '^'),
        ('7', '&'),
        ('8', '*'),
        ('9', '('),
        ('0', ')'),
        ('-', '_'),
        ('=', '+'),
        ('[', '{'),
        (']', '}'),
        ('\\', '|'),
        (';', ':'),
        ('\'', '"'),
        (',', '<'),
        ('.', '>'),
        ('/', '?'),
        ('`', '~'),
    ];

    // Backspace is sometimes interpreted as char 8, delete as 7F, tab as 9 and escape as 1B
    if key == '\u{8}' || key == '\u{7F}' {
        return vga::delete_prev_char();
    } else if key == '\u{9}' || key == '\u{1B}' {
        return;
    }
    
    // Convert the key to it's non-shift form, to counter pc-keyboard's broken shift translation
    let shifted = if let Some(shift) = SHIFT_KEYS.iter().find(|s| s.0 == key || s.1 == key) {
        key = shift.0;
        Some(shift.1)
    } else {
        key.make_ascii_lowercase();
        None
    };

    // If shift is held
    let shift = SHIFT.load(Ordering::Relaxed) != 0;

    // Print the key in either shift, caps or regular form
    if let Some(shifted) = shifted
        && shift
    {
        print!("{shifted}")
    } else if kbd.capslock ^ shift {
        print!("{}", key.to_ascii_uppercase())
    } else {
        print!("{key}")
    }
}
