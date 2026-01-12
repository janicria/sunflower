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
    kernel/src/interrupts.rs

    The interrupts module handles exceptions and irqs.
    This file is responsible for nothing really :(

    Contains 3 submodules:
    * idt.rs - Handles loading the IDT and it's handlers
    * keyboard.rs - PS/2 keyboard driver, TODO: move out of the interrupts module
    * pic.rs - Initialises the PICs
*/

use crate::{exit_on_err, startup::ExitCode, time, vga::cursor};
use core::{arch::asm, fmt::Display};
use idt::InterruptDescriptor;
pub use keyboard::init as init_kbd;
use libutil::{InitLater, LoadRegisterError, TableDescriptor};
pub use pic::init as init_pic;

mod idt;
mod keyboard;
mod pic;

/// Where IRQ vectors start in the IDT.
static IRQ_START: usize = 32;

/// The loaded IDT.
pub static IDT: InitLater<Idt> = InitLater::uninit();

/// The Interrupt Descriptor Table.
#[derive(Debug)]
#[repr(transparent)]
pub struct Idt([InterruptDescriptor; 256]);

/// The interrupt stack frame.
#[derive(Debug, Default)]
#[repr(C)]
pub struct IntStackFrame {
    ip: u64,
    cs: u64,
    flags: u64,
    sp: u64,
    ss: u64,
}

impl Display for IntStackFrame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "Stack frame for 0x{:x}: flags: {}\n      CS: {:x} | CP: {:x} | SS: {:x}",
            self.ip, self.flags, self.cs, self.sp, self.ss
        )
    }
}

/// Loads the IDT into the `IDT` static.
///
/// # Safety
/// Only run this once, early into startup.
pub unsafe fn load_idt() -> ExitCode<LoadRegisterError<Idt>> {
    let idt = exit_on_err!(IDT.init(Idt::new()), Stop);
    dbg_info!("IDT loaded at 0x{:x}", idt as *const Idt as u64);

    // Safety: Using properly filled out IDT.
    let loaded_idt = unsafe { idt.load() };

    // Return Err if sidt (store IDT) != descriptor passed to lidt
    if idt_register() != loaded_idt {
        return ExitCode::Stop(LoadRegisterError::Store("IDT"));
    }

    ExitCode::Ok
}

/// Returns the current value in the IDT register.
pub fn idt_register() -> TableDescriptor<Idt> {
    let mut idt = TableDescriptor::invalid();
    // Safety: We're just storing a value
    unsafe { asm!("sidt [{}]", in(reg) (&mut idt), options(preserves_flags, nostack)) };
    idt
}

/// Repeatedly loops polling the keyboard.
pub fn kbd_poll_loop() -> ! {
    loop {
        keyboard::poll_keyboard();

        // Repeatedly busy waiting for a new key to be pressed is stupidly inefficient,
        // since the PIT fires an interrupt every ms anyway, why not just hlt after each poll?
        unsafe { asm!("hlt") }

        // Update the cursor every 100 ms
        if time::get_time().is_multiple_of(10) {
            cursor::update_visual_pos();
        }
    }
}

/// Waits for the user to type either `y` or `n`.
///
/// Loops forever if the keyboard failed to initialise.
pub fn kbd_wait_for_response(prompt: &str, enter_eq_y: bool) -> bool {
    if enter_eq_y {
        print!("{prompt}? [Y/n] ");
    } else {
        print!("{prompt}? [y/N] ")
    }

    cursor::update_visual_pos();
    let result = keyboard::wait_for_response(enter_eq_y);
    if result {
        println!("y");
    } else {
        println!("n")
    }
    result
}

/// Sets external interrupts.
pub fn sti() {
    unsafe { asm!("sti") }
}

/// Clears external interrupts.
pub fn cli() {
    unsafe { asm!("cli") }
}

/// Halts the CPU.
pub fn hlt() {
    // Safety: Just halting
    unsafe { asm!("hlt") }
}

/// Causes a triple fault.
/// Can be used as the stupidest way ever to restart the device.
pub fn triple_fault() {
    // Safety: We're deliberately being very unsafe here
    unsafe {
        let descriptor = TableDescriptor::<Idt>::invalid();
        asm!("lidt ({0})", in(reg) &descriptor, options(att_syntax)); // load invalid descriptor
        asm!("int 0x42") //  gpf -> double fault -> triple fault
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that various structs passed to the CPU are the size that the CPU expects them.
    #[test_case]
    fn structs_have_the_right_size() {
        assert_eq!(size_of::<IntStackFrame>(), 40);
        assert_eq!(size_of::<InterruptDescriptor>(), 16);
        assert_eq!(size_of::<Idt>(), size_of::<InterruptDescriptor>() * 256);
    }
}
