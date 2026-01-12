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
    kernel/src/main.rs

    The kernel's entry point
*/

#![no_std]
#![no_main]
#![test_runner(tests::run_tests)]
#![reexport_test_harness_main = "tests"]
#![forbid(static_mut_refs)] // clippy::undocumented_unsafe_blocks)]
#![feature(
    abi_x86_interrupt,
    sync_unsafe_cell,
    yeet_expr,
    custom_test_frameworks,
    trim_prefix_suffix
)]
#![allow(
    clippy::unusual_byte_groupings,
    clippy::deref_addrof,
    clippy::identity_op
)]

#[macro_use]
mod vga;
mod floppy;
mod gdt;
mod interrupts;
mod panic;
mod ports;
mod speaker;
#[macro_use]
mod startup;
#[macro_use]
mod sysinfo;
#[cfg(test)]
mod tests;
mod time;

// Warn anyone just running `cargo build` to use seeder tool
#[cfg(any(debug_assertions, not(feature = "bootimage")))]
compile_error!(
    "Please build sunflower using seeder, run `cargo sdr help` in the main sunflower directory for help"
);

/// The kernel entry point.
/// # Safety
/// Please don't run the kernel twice.
#[unsafe(export_name = "_start")]
pub unsafe extern "C" fn kmain() -> ! {
    // Safety: Considering that this is the kernel entry point,
    // I'm pretty sure these startup tasks are only being ran once
    unsafe {
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
        startup::run("Initialised floppy drive", floppy::init_wrapper);
        startup::run("Initialised floppyfs", floppy::floppyfs::init_floppyfs);
    }

    #[cfg(test)]
    tests();

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
