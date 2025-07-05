#![no_std]
#![no_main]
#![allow(static_mut_refs, internal_features)]
#![feature(core_intrinsics)]

use core::panic::PanicInfo;

/// Allows writing to the VGA text buffer
#[macro_use]
mod vga;
/// Handles various interrupts
#[macro_use]
mod interrupts;
/// Handles writing to and reading from specific I/O ports
mod ports;
/// Allows playing sounds through the PC speaker
mod speaker;
/// Controls the current state of the kernel
mod state;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    vga::init();
    interrupts::init();
    unsafe { state::FLAGS &= 0b1111_1110 } // startup = false

    use vga::Color;
    vga::print_color("All startup tasks completed! ", Color::Green);
    unsafe { vga::WRITER.write_char(1, Color::Green, Color::Black) }; // happy face
    println!("\n");
    speaker::play_chime();

    // speaker::play_song();
    // panic!("Something bad happened!");
    // unsafe { core::arch::asm!("int3") } // breakpoint
    // unsafe { core::arch::asm!("int 42") } // gpf
    // interrupts::triple_fault();
    // unsafe { core::arch::asm!("ud2"); }   // invalid op
    // unsafe { *(0x8 as *mut u64) = 42 }; // page fault

    idle()
}

/// Enters 'idle' mode.
fn idle() -> ! {
    unsafe {
        state::FLAGS |= 0b0000_0010; // idle = true
        loop {
            core::arch::asm!("sti; hlt; cli");
        }
    }
}

/// Waits for `ticks` ticks.
fn wait(ticks: u64) {
    unsafe {
        let char = &mut *(0xb809e as *mut u16);
        let target_time = state::TIME + ticks;
        while state::TIME < target_time {
            core::arch::asm!("sti; hlt; cli");

            // Make waiting char toggle colors
            if state::TIME % 9 == 0 {
                const ON: u16 = 1025;
                const OFF: u16 = 3841;
                if *char == ON { *char = OFF } else { *char = ON }
            }
        }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    unsafe { state::PANICS += 1 };
    println!(
        "PANIC OCCURRED: {}\nLocation: {}",
        info.message(),
        info.location().unwrap() // always succeeds
    );

    speaker::hold_duration(400, 8);
    state::ohnonotgood();
    idle()
}
