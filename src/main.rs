#![no_std]
#![no_main]
#![allow(internal_features, clippy::unusual_byte_groupings)]
#![forbid(static_mut_refs)]
#![feature(core_intrinsics, abi_x86_interrupt)]

/// Allows writing to the VGA text buffer
#[macro_use]
mod vga;

/// Handles various interrupts
mod interrupts;

/// Handles writing to and reading from specific I/O ports
mod ports;

/// Allows playing sounds through the PC speaker
mod speaker;

/// Handles the PIT.
mod time;

// Warn anyone just running `cargo build` to use the bootimage tool
#[cfg(any(debug_assertions, not(feature = "bootimage")))]
compile_error!(
    "Please build sunflower using `cargo b` or `cargo bootimage` and 
run it using either `run-nosound` `run-pipewire` or `run-pulseaudio`"
);

/// The kernel entry point.
/// # Safety
/// This function assumes it's being run immediately 
/// after taking control from the bootloader.
#[unsafe(export_name = "_start")]
pub unsafe extern "C" fn kmain() -> ! {
    vga::init();
    interrupts::init();
    time::set_timer_interval();
    vga::print_color("All startup tasks completed! \u{1}\n\n", vga::Color::Green);
    vga::update_vga_cursor();
    speaker::play_chime();

    // Test various errors
    // unsafe { core::arch::asm!("int3") }         // breakpoint
    // unsafe { core::arch::asm!("ud2") }          // invalid op
    // panic!("Something bad happened!");          // panic
    // unsafe { core::arch::asm!("int 42") }       // gpf
    // unsafe { *core::ptr::dangling_mut() = 42 }; // page fault
    // unsafe { core::arch::asm!("int 8") }        // double fault

    loop {
        unsafe { core::arch::asm!("hlt") }
    }
}
