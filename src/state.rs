use crate::vga::{self, Color};

/// Bit 0 - In startup mode
/// But 1 - Idle mode
/// Bit 2 - Printing (you would not believe how many errors occur while printing)
/// Bit 3 - Everything has gone wrong
/// Bit 4 - Return from everything has gone wrong
pub static mut FLAGS: u8 = 0b0000_0001;

/// How many times the kernel has panicked
pub(super) static mut PANICS: u8 = 0;

/// The last exception which occurred
#[unsafe(no_mangle)]
pub static mut PREV_EXP: i8 = -1;

/// The second last exception which occurred
#[unsafe(no_mangle)]
pub static mut SECOND_LAST_EXP: i8 = -1;

/// How many ticks the kernel has been running for.
/// Increases every ~18.2 Hz
pub static mut TIME: u64 = 0;

/// Ran when an 'unrecoverable' error occurs.
#[unsafe(no_mangle)]
pub(super) extern "C" fn ohnonotgood() {
    unsafe {
        FLAGS |= 0b0000_1000; // everything has gone wrong = true

        vga::print_color("An unrecoverable error has occurred!\n", Color::LightRed);

        println!(
            "System flags: {startup}{idle}{printing}({FLAGS})
Uptime: {TIME}. Panics: {PANICS}. Previous / second last exception IDs: {PREV_EXP} / {SECOND_LAST_EXP}
Press 1 to reboot the system, 2 to continue execution, 3 to enter idle mode.",
            startup = bit_eq!(FLAGS, 1, "Startup, ", ""),
            idle = bit_eq!(FLAGS, 2, "Idle, ", ""),
            printing = bit_eq!(FLAGS, 3, "Printing, ", ""),
        );

        core::arch::asm!("sti");

        loop {
            // Return from ehgw equals true
            if FLAGS == FLAGS | 0b0001_0000 {
                FLAGS &= 0b1110_0011; // ehgw, return from ehgw and printing =  false
                return;
            }
        }
    }
}
