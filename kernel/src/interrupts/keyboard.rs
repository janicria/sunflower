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

use core::fmt::Display;
use core::hint;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use pc_keyboard::layouts::Us104Key;
use pc_keyboard::{
      DecodedKey, HandleControl, KeyCode, KeyEvent, KeyState, Keyboard,
      Modifiers, ScancodeSet2,
};
use ps2::Controller;
use ps2::error::KeyboardError;
use ps2::flags::{ControllerConfigFlags, KeyboardLedFlags};

use crate::ports::{self, Port};
use crate::startup::{self, ExitCode};
use crate::sysinfo::SystemInfo;
use crate::vga::buffers::{self, BUFFER_HEIGHT};
use crate::vga::cursor::{CursorPos, CursorShift, shift_cursor};
use crate::vga::{self, print};
use crate::{PANIC, speaker};

/// Circular scancode buffer where each AtomicU8 represents a scancode.
// The genius idea of this buffer was taken from the below video
// (it's such a good idea I'd feel bad not crediting it):
// https://www.youtube.com/watch?v=dL0GO9SeBh0
static KBD_BUF: [AtomicU8; 256] = [const { AtomicU8::new(0) }; 256];

/// Index into the last handled scancode in the keyboard buffer.
static KBD_RPTR: AtomicU8 = AtomicU8::new(0);

/// Index into the last added scancode to the keyboard buffer.
static KBD_WPTR: AtomicU8 = AtomicU8::new(0);

/// The last value read from port 0x60.
static PREV_RESPONSE: AtomicU8 = AtomicU8::new(0);

/// The current state of the shift keys.
/// * Bit 0 - Left shift held
/// * Bit 1 - Right shift held
static SHIFT: AtomicU8 = AtomicU8::new(0);

/// Whether SYSRQ is being held or not.
static SYSRQ: AtomicBool = AtomicBool::new(false);

/// Mapping of how to translate keys when shift is held.
static SHIFT_MAPPING: [(char, char); 21] = [
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

/// Disables mouse, runs some tests, sets config, then sets the scancode and
/// numlock LEDs.
/// # Safety
/// Ports `0x60` & `0x64` must not be used anywhere else.
pub unsafe fn init() -> ExitCode<KbdInitError> {
      super::sti();

      if !startup::PIC_INIT.load() {
            return ExitCode::Error(KbdInitError::new("The PIC ins't init!"));
      }

      // Safety: This is the only use of ports 0x60 & 0x64
      let mut controller = unsafe { Controller::new() };

      macro_rules! parse_err {
            ($msg: expr, $res: expr) => {
                  if let Err(e) = $res {
                        let err = KbdInitError {
                              msg:     $msg,
                              kbd_err: Some(e.into()),
                        };
                        return ExitCode::Error(err);
                  }
            };
      }

      parse_err!("Disable keyboard", controller.disable_keyboard());
      parse_err!("Disable mouse", controller.disable_mouse());
      _ = controller.read_data(); // ignore result as it's just a buffer flush
      parse_err!("Controller test", controller.test_controller());
      parse_err!("Keyboard test", controller.test_keyboard());

      let mut cfg = ControllerConfigFlags::all();
      cfg.set(ControllerConfigFlags::DISABLE_KEYBOARD, false);
      cfg.set(ControllerConfigFlags::ENABLE_MOUSE_INTERRUPT, false);
      // so set 2 is actually set 2
      cfg.set(ControllerConfigFlags::ENABLE_TRANSLATE, false);
      parse_err!("Set config", controller.write_config(cfg));

      let mut kbd = controller.keyboard();
      parse_err!("Keyboard Echo", kbd.echo()); // echo!!
      parse_err!("Set scancode", kbd.set_scancode_set(2));
      parse_err!("Set LEDS", kbd.set_leds(KeyboardLedFlags::NUM_LOCK));
      parse_err!("Reset keyboard", kbd.reset_and_self_test());

      // Safety: We just initialised it above
      unsafe { startup::KBD_INIT.store(true) }

      ExitCode::Ok
}

/// Error returned from `init`.
pub struct KbdInitError {
      msg:     &'static str,
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
      /// The scancode for `y` in set 2.
      const Y_SCANCODE: u8 = 0x35;

      /// The scancode for `n` in set 2.
      const N_SCANCODE: u8 = 0x31;

      /// The scancode for `enter` in set 2.
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
/// Should only be ran inside of the PS/2 keyboard interrupt handler.
#[unsafe(no_mangle)]
unsafe fn kbd_handler() {
      if !startup::KBD_INIT.load() {
            return;
      }

      super::cli();

      // Safety: It's ok to read port 0x60 inside of an interrupt handler here,
      // as besides from in startup, it's never accessed in the 'main' execution
      // of code, also the above cli prevents any other int handlers
      let scancode = unsafe { ports::readb_nodummy(Port::PS2Data) };
      let ptr = KBD_WPTR.load(Ordering::Relaxed) as usize;

      KBD_WPTR.fetch_add(1, Ordering::Relaxed);
      KBD_BUF[ptr].store(scancode, Ordering::Relaxed);
      PREV_RESPONSE.store(scancode, Ordering::Relaxed);

      super::sti();
}

/// Runs the corresponding action if any syscmds were inputted.
fn check_syscmd(key: KeyCode, kbd: &Modifiers) {
      if !(SYSRQ.load(Ordering::Relaxed) || kbd.is_ctrl() && kbd.is_alt()) {
            return;
      }

      match key {
            KeyCode::F1 => print_sysinfo(),
            KeyCode::F2 => {
                  buffers::clear();
                  vga::draw_topbar();
            }
            KeyCode::F3 => speaker::play_song(),
            KeyCode::F4 => PANIC!(badbug "Triggered System Command 4 \
                                  by pressing Ctrl+Alt+F4 or SysRq+F4"),
            KeyCode::F5 => super::triple_fault(),
            KeyCode::F6 => buffers::swap(),
            KeyCode::F7 => print_help(),
            _ => (),
      }

      fn print_sysinfo() {
            buffers::swap();
            buffers::clear();
            vga::draw_topbar();

            println!(fg = LightBlue, "\nSystem information");
            print!("{}", SystemInfo::now());

            CursorPos::set_col(0);
            CursorPos::set_row(BUFFER_HEIGHT - 1);
            print!("Previous screen stored in alt buffer (Use SysCmd 6)")
      }

      fn print_help() {
            buffers::swap();
            buffers::clear();
            vga::draw_topbar();

            println!(fg = Pink, "\nWelcome to Sunflower!! \u{1}");
            println!(
                  "Sunflower is a smallish kernel written by me (janicria) \
            using Rust and some\ninline assembly. It's open source, and's \
            repository can be found at\n
                    https://github.com/janicria/sunflower\n\n\
            Right now there isn't really any form of user input besides from \
            some basic\nASCII screen drawing, but there are some builtin \
            keyboard shortcuts which can\nbe ran using Ctrl+Alt+FX, SysRq+FX \
            or PrtScr+FX, where X can be either:\n
         1 - Prints system information   2 - Clears the screen
         3 - Beeps the PC speaker        4 - Triggers a kernel panic
         5 - Restarts the device         6 - Swaps between text buffers
         7 - Shows this help message"
            );
      }
}

/// Ran by the keyboard poll loop whenever a key is pressed.
fn keyboard_input(
      scancode: u8, event: &KeyEvent,
      kbd: &mut Keyboard<Us104Key, ScancodeSet2>,
) {
      const LSHIFT_SCANCODE: u8 = 0x12;
      const RSHIFT_SCANCODE: u8 = 0x59;
      const SYSRQ_SCANCODE: u8 = 0x7F;
      const SYSRQ_SCANCODE_ALT: u8 = 0x7C;

      // We can't just flip the bit if a key is pressed or released, as
      // doing so would cause holding a key while launching QEMU make the
      // key stuck in the opposite state.
      if event.state == KeyState::Down {
            if scancode == LSHIFT_SCANCODE {
                  SHIFT.fetch_or(1 << 0, Ordering::Relaxed);
            } else if scancode == RSHIFT_SCANCODE {
                  SHIFT.fetch_or(1 << 1, Ordering::Relaxed);
            } else if scancode == SYSRQ_SCANCODE ||
                  scancode == SYSRQ_SCANCODE_ALT
            {
                  SYSRQ.store(true, Ordering::Relaxed);
            }
      }

      if event.state == KeyState::Up {
            if scancode == LSHIFT_SCANCODE {
                  SHIFT.fetch_and(!(1 << 0), Ordering::Relaxed);
            } else if scancode == RSHIFT_SCANCODE {
                  SHIFT.fetch_and(!(1 << 1), Ordering::Relaxed);
            } else if scancode == SYSRQ_SCANCODE ||
                  scancode == SYSRQ_SCANCODE_ALT
            {
                  SYSRQ.store(false, Ordering::Relaxed);
            }
      }

      if let Some(key) = kbd.process_keyevent(event.clone()) {
            let mods = kbd.get_modifiers();
            check_syscmd(event.code, mods);

            match key {
                  DecodedKey::RawKey(key) => check_arrows(key),
                  DecodedKey::Unicode(key) => print_char(key, mods),
            }
      }

      fn check_arrows(key: KeyCode) {
            match key {
                  KeyCode::ArrowLeft => shift_cursor(CursorShift::Left),
                  KeyCode::ArrowRight => shift_cursor(CursorShift::Right),
                  KeyCode::ArrowUp => shift_cursor(CursorShift::Up),
                  KeyCode::ArrowDown => shift_cursor(CursorShift::Down),
                  _ => (),
            }
      }

      #[rustfmt::skip]
      fn print_char(mut key: char, kbd: &Modifiers) {
            if key == '\u{8}' || key == '\u{7F}' { // backspace / delete
                  return print::delete_prev_char();
            } else if key == '\u{9}' || // tab
            key == '\u{1B}' || // escape
            (key == '\n' && cfg!(feature = "disable_enter"))
            {
                  return;
            }

            let shift_held = SHIFT.load(Ordering::Relaxed) != 0;

            // Convert to non-shift form to counter
            // pc-keyboard's broken shift translation
            let shifted = if let Some(shift) =
                  SHIFT_MAPPING.iter().find(|s| s.0 == key || s.1 == key)
            {
                  key = shift.0;
                  Some(shift.1)
            } else {
                  key.make_ascii_lowercase();
                  None
            };

            if let Some(shifted) = shifted &&
                  shift_held
            {
                  print!("{shifted}")
            } else if kbd.capslock ^ shift_held {
                  print!("{}", key.to_ascii_uppercase())
            } else {
                  print!("{key}")
            }
      }
}

/// Polls the keyboard buffer for any new keys pressed.
pub fn poll_keyboard() {
      /// The current state of the keyboard
      static mut KBD: Keyboard<Us104Key, ScancodeSet2> =
            Keyboard::new(ScancodeSet2::new(), Us104Key, HandleControl::Ignore);

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

      if let Ok(event) = kbd.add_byte(scancode) &&
            let Some(ref event) = event
      {
            keyboard_input(scancode, event, kbd);
      }
}
