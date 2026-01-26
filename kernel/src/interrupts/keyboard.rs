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
    kernel/src/interrupts/keyboard.rs

    PS/2 keyboard driver - should be moved out of the interrupts module.
    Contained within the interrupts module
*/

use crate::{
    PANIC,
    ports::{self, Port},
    speaker,
    startup::{self, ExitCode},
    sysinfo::SystemInfo,
    vga::{
        self,
        buffers::{self, BUFFER_HEIGHT},
        cursor::{self, CursorPos, CursorShift},
        print,
    },
};
use core::{
    fmt::Display,
    hint,
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
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

/// The last value read from port 0x60.
static PREV_RESPONSE: AtomicU8 = AtomicU8::new(0);

/// Whether shift is being held or not. Used as pc-keyboard's shift check is dodgy.
///
/// - Bit 0 - Left shift
/// - Bit 1 - Right shift
static SHIFT: AtomicU8 = AtomicU8::new(0);

/// Whether SYSRQ is being held or not.
static SYSRQ: AtomicBool = AtomicBool::new(false);

/// Disables mouse, runs some tests, sets config, then sets the scancode and numlock LEDs.
/// # Safety
/// Ports `0x60` & `0x64` must not be used anywhere else.
pub unsafe fn init() -> ExitCode<KbdInitError> {
    super::sti();

    if !startup::PIC_INIT.load() {
        return ExitCode::Error(KbdInitError::new("The PIC ins't init!"));
    }

    // Safety: This is the only use of ports 0x60 & 0x64, excluding unsafe functions
    let mut controller = unsafe { Controller::new() };

    macro_rules! parse_err {
        ($msg: expr, $res: expr) => {
            match $res {
                Err(e) => {
                    return ExitCode::Error(KbdInitError {
                        msg: $msg,
                        kbd_err: Some(e.into()),
                    })
                }
                Ok(_) => (),
            }
        };
    }

    // Disable devices
    parse_err!("Disable keyboard", controller.disable_keyboard());
    parse_err!("Disable mouse", controller.disable_mouse());

    // It doesn't matter if it's an err since we're just flushing the buffer
    _ = controller.read_data();

    // Tests
    parse_err!("Controller test", controller.test_controller());
    parse_err!("Keyboard test", controller.test_keyboard());

    // Config
    let mut cfg = ControllerConfigFlags::all();
    cfg.set(ControllerConfigFlags::DISABLE_KEYBOARD, false); // enable kbd
    cfg.set(ControllerConfigFlags::ENABLE_MOUSE_INTERRUPT, false); // since we don't use the mouse
    cfg.set(ControllerConfigFlags::ENABLE_TRANSLATE, false); // so scancode set 2 is actually scancode set 2
    parse_err!("Set config", controller.write_config(cfg));

    // Echo!!
    let mut kbd = controller.keyboard();
    parse_err!("Keyboard Echo", kbd.echo());

    // Scancode set 2 & Num Lock LEDs
    parse_err!("Set scancode", kbd.set_scancode_set(2));
    parse_err!("Set LEDS", kbd.set_leds(KeyboardLedFlags::NUM_LOCK));
    parse_err!("Reset keyboard", kbd.reset_and_self_test());

    // Safety: We just initialised it above
    unsafe { startup::KBD_INIT.store(true) }

    ExitCode::Ok
}

/// Error returned from `init`.
pub struct KbdInitError {
    msg: &'static str,
    kbd_err: Option<KeyboardError>,
}

impl KbdInitError {
    /// Returns a new error without the `kbd_err` field.
    fn new(msg: &'static str) -> Self {
        KbdInitError { msg, kbd_err: None }
    }
}

impl Display for KbdInitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.msg)?;

        if let Some(ref err) = self.kbd_err {
            write!(f, " - {err:?}")?;
        }

        Ok(())
    }
}

/// Waits for either `y`, `n` or `enter` to be pressed.
#[cfg_attr(test, allow(unused))]
pub fn wait_for_response(enter_eq_true: bool) -> bool {
    /// The scancode for `y` in scancode set 2.
    const Y_SCANCODE: u8 = 0x35;

    /// The scancode for `n` in scancode set 2.
    const N_SCANCODE: u8 = 0x31;

    /// The scancode for `enter` in scancode set 2.
    const ENTER_SCANCODE: u8 = 0x5A;

    #[cfg(test)]
    {
        println!("Cannot ask for input in a test!");
        crate::tests::exit_qemu(true)
    }

    loop {
        hint::spin_loop(); // pause instruction
        match PREV_RESPONSE.load(Ordering::Relaxed) {
            Y_SCANCODE => return true,
            N_SCANCODE => return false,
            ENTER_SCANCODE => return enter_eq_true,
            _ => (),
        }
    }
}

/// Adds the last response from the keyboard to the keyboard buffer.
/// # Safety
/// Reads from port 0x60 for it's response.
#[unsafe(no_mangle)]
unsafe fn kbd_handler() {
    if !startup::KBD_INIT.load() {
        return;
    }

    // Prevent another IRQ handler from adding to the buffer at the same time.
    super::cli();

    // Safety: The caller must ensure that it's safe to read from port 0x60
    let scancode = unsafe { ports::readb(Port::PS2Data) };
    let ptr = KBD_WPTR.load(Ordering::Relaxed) as usize;

    // Save the scancode to the buffer
    KBD_WPTR.fetch_add(1, Ordering::Relaxed);
    KBD_BUF[ptr].store(scancode, Ordering::Relaxed);
    PREV_RESPONSE.store(scancode, Ordering::Relaxed);

    super::sti();
}

/// Polls the keyboard buffer for any new keys pressed.
pub fn poll_keyboard() {
    /// The current state of the keyboard
    static mut KBD: Keyboard<Us104Key, ScancodeSet2> =
        Keyboard::new(ScancodeSet2::new(), Us104Key, HandleControl::Ignore);

    // Left and right shift scancodes in set 2.
    static LSHIFT_SCANCODE: u8 = 0x12;
    static RSHIFT_SCANCODE: u8 = 0x59;

    // Sys request scancodes in set 2.
    static SYSRQ_SCANCODE: u8 = 0x7F;
    static SYSRQ_SCANCODE_ALT: u8 = 0x7C;

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

    // If a key was pressed
    if let Ok(event) = kbd.add_byte(scancode)
        && let Some(ref event) = event
    {
        // Handle shift and sys request pressed
        // We can't just flip the bit if the key is pressed OR released above, as holding one of the keys
        // while launching QEMU (or any other VM I assume) causes the key  to be permanently stuck in the
        // opposite state, as sunflower sees a key is released, and sets the bit, when it should be cleared.
        if event.state == KeyState::Down {
            if scancode == LSHIFT_SCANCODE {
                SHIFT.fetch_or(1 << 0, Ordering::Relaxed);
            } else if scancode == RSHIFT_SCANCODE {
                SHIFT.fetch_or(1 << 1, Ordering::Relaxed);
            } else if scancode == SYSRQ_SCANCODE || scancode == SYSRQ_SCANCODE_ALT {
                SYSRQ.store(true, Ordering::Relaxed);
            }
        }

        // Handle shift and sys request released
        if event.state == KeyState::Up {
            if scancode == LSHIFT_SCANCODE {
                SHIFT.fetch_and(!(1 << 0), Ordering::Relaxed);
            } else if scancode == RSHIFT_SCANCODE {
                SHIFT.fetch_and(!(1 << 1), Ordering::Relaxed);
            } else if scancode == SYSRQ_SCANCODE || scancode == SYSRQ_SCANCODE_ALT {
                SYSRQ.store(false, Ordering::Relaxed);
            }
        }

        if let Some(key) = kbd.process_keyevent(event.clone()) {
            let mods = kbd.get_modifiers();
            system_command(event.code, mods);

            match key {
                DecodedKey::RawKey(key) => handle_arrows(key),
                DecodedKey::Unicode(key) => print_key(key, mods),
            }
        }
    }
}

/// Checks if any system commands were run and runs the corresponding action if so.
fn system_command(key: KeyCode, kbd: &Modifiers) {
    // If Ctrl + Alt or SysRq is held
    if (kbd.is_ctrl() && kbd.is_alt()) || SYSRQ.load(Ordering::Relaxed) {
        match key {
            KeyCode::F1 => print_sysinfo(),
            KeyCode::F3 => speaker::play_song(),
            KeyCode::F4 => {
                PANIC!(badbug "Triggered System Command 4 by pressing Ctrl+Alt+F4 or SysRq+F4")
            }
            KeyCode::F5 => super::triple_fault(),
            KeyCode::F6 => buffers::swap(),
            KeyCode::F7 => print_help(),
            KeyCode::F2 => {
                buffers::clear();
                vga::draw_topbar();
            }
            _ => (),
        }
    }
}

/// Used by syscmd 1 to print the system info.
fn print_sysinfo() {
    // Store prev buffer in alt
    buffers::swap();
    buffers::clear();
    vga::draw_topbar();

    println!(fg = LightBlue, "\nSystem information");
    print!("{}", SystemInfo::now());

    // Print message in bottom left
    CursorPos::set_col(0);
    CursorPos::set_row(BUFFER_HEIGHT - 1);
    print!("Previous screen stored in alt buffer (Use SysCmd 6)")
}

/// Used by syscmd 7 to print the system info.
fn print_help() {
    // Store prev buffer in alt
    buffers::swap();
    buffers::clear();
    vga::draw_topbar();

    println!(fg = Pink, "\nWelcome to Sunflower!! \u{1}");

    // Explains what syscmds are
    println!(fg = LightBlue, "\nHow to run System Commands");
    print!("Sunflower supports some keyboard shortcuts, known as System Commands or SysCmds.");
    println!(
        "Hold either Ctrl+Alt+FX or SysRq+FX, to run system command X.
Note: The SysRq key might be the same as PrintScreen on your keyboard."
    );

    // System commands list
    println!(fg = LightBlue, "\nAvailable System Commands");
    println!(
        "1 - Prints system information   2 - Clears the screen
3 - Beeps loudly                4 - Crashes sunflower via rbod
5 - Restarts the device         6 - Swap between text buffers
7 - Shows this help message"
    );

    // Talks about sunflower being a glorified text editor
    println!(fg = LightBlue, "\nDrawing");
    println!(
        "By using the arrow keys, you can position the cursor to anywhere on the screen.
You can write or draw whatever you want, by typing characters on your keyboard."
    );
}

/// Handles when an arrow key is pressed.
fn handle_arrows(key: KeyCode) {
    match key {
        KeyCode::ArrowLeft => cursor::shift_cursor(CursorShift::Left),
        KeyCode::ArrowRight => cursor::shift_cursor(CursorShift::Right),
        KeyCode::ArrowUp => cursor::shift_cursor(CursorShift::Up),
        KeyCode::ArrowDown => cursor::shift_cursor(CursorShift::Down),
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
        return print::delete_prev_char();
    } else if key == '\u{9}' || key == '\u{1B}' {
        return;
    }

    // Disable enter feature
    if key == '\n' && cfg!(feature = "disable_enter") {
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
