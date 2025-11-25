#![no_std]
#![no_main]
#![test_runner(tests::run_tests)]
#![reexport_test_harness_main = "tests"]
#![forbid(static_mut_refs)] // clippy::undocumented_unsafe_blocks)]
#![feature(abi_x86_interrupt, sync_unsafe_cell, yeet_expr, custom_test_frameworks)]
#![allow(
    clippy::unusual_byte_groupings,
    clippy::deref_addrof,
    clippy::identity_op
)]

/// Allows writing to the VGA text buffer
#[macro_use]
mod vga;

mod floppy;

/// Allows interacting with files and directories.
mod fs;

/// Handles loading a new TSS & GDT.
mod gdt;

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

/// Handles running tests and writing to serial ports.
#[cfg(test)]
mod tests;

/// Handles the PIT.
mod time;

// Warn anyone just running `cargo build` to use the bootimage tool
#[cfg(any(debug_assertions, not(feature = "bootimage")))]
compile_error!(
    "Please build sunflower using `cargo b` or `cargo bootimage` 
run it using `cargo run-nosound` `cargo run-pipewire` or `cargo run-pulseaudio` 
use clippy via `cargo paperclip` and test using `cargo did-i-break-anything`"
);

/// The kernel entry point.
/// # Safety
/// Please don't run the kernel twice.
#[unsafe(export_name = "_start")]
pub unsafe extern "C" fn kmain() -> ! {
    startup::run("Connected VGA", vga::init);
    startup::run("Loaded IDT", interrupts::load_idt);
    startup::run("Prepared TSS load", gdt::setup_tss);
    startup::run("Loaded GDT", gdt::load_gdt);
    startup::run("Finished TSS load", gdt::load_tss);
    startup::run("Initialised PIC", interrupts::init_pic);
    startup::run("Prepared RTC sync", time::setup_rtc_int);
    startup::run("Set PIT frequency", time::set_timer_interval);
    startup::run("Initialised keyboard", interrupts::init_kbd);
    startup::run("Checked CPUID", sysinfo::check_cpuid);
    startup::run("Finished RTC sync", time::wait_for_rtc_sync);
    startup::run("Initialised floppy drive", floppy::init);
    startup::run("Mounted floppy drive", fs::init_floppyfs);

    #[cfg(test)]
    tests();

    if cfg!(feature = "debug_info") {
        let ptr = fs::alloc_inode(8, unsafe { core::mem::zeroed() }, 1).unwrap();
        let mut buf = [0; 512];
        let c = fs::read_inode(ptr, &mut buf).unwrap();
        println!("read {c} from {ptr}");
    }

    vga::draw_topbar("Sunflower");
    println!(fg = Green, "\nAll startup tasks completed! \u{1}\n");
    vga::cursor::update_visual_pos();
    speaker::play_chime();
    interrupts::kbd_poll_loop()
}

/// Hangs forever, never returning.
/// Only use this when you have to.
#[unsafe(no_mangle)]
#[unsafe(naked)]
extern "C" fn hang() -> ! {
    core::arch::naked_asm!(
        "cli",                         // disable ints to make sure nothing else is run
        "mov rbx, 0xDeadDeadDeadDead", // pseudo error message which can be viewed in QEMU
        "hlt",                         // save power by halting
        "jmp hang"                     // halt can get bypassed by a NMI or System Management Mode
    )
}
