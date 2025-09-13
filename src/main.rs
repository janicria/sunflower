#![no_std]
#![no_main]
#![allow(clippy::unusual_byte_groupings, clippy::deref_addrof)]
#![forbid(static_mut_refs)]
#![feature(abi_x86_interrupt)]

/// Allows writing to the VGA text buffer
#[macro_use]
mod vga;

/// Handles various interrupts
mod interrupts;

/// Handles writing to and reading from specific I/O ports
mod ports;

/// Allows playing sounds through the PC speaker
mod speaker;

/// Handles post-boot startup tasks.
mod startup;

/// Handles system information.
mod sysinfo;

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
/// Please don't run the kernel twice.
#[unsafe(export_name = "_start")]
pub unsafe extern "C" fn kmain() -> ! {
    startup::run("Connected VGA", vga::init);
    startup::run("Loaded IDT", interrupts::load_idt);
    startup::run("Initialised PIC", interrupts::init_pic);
    startup::run("Set PIT frequency", time::set_timer_interval);
    startup::run("Initialised keyboard", interrupts::init_kbd);
    startup::run("Prepared for RTC sync", time::setup_rtc_int);
    startup::run("Checked CPUID", sysinfo::check_cpuid);

    startup::SYS_INIT.store(true, core::sync::atomic::Ordering::Relaxed);
    vga::print_color("All startup tasks completed! \u{1}\n\n", vga::Color::Green);
    vga::update_vga_cursor();
    speaker::play_chime();
    interrupts::kbd_poll_loop()
}
