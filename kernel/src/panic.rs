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
    kernel/src/panic.rs

    Handles kernel panics and the [`PANIC!`] macro.
*/

use crate::{
    floppy::motor,
    interrupts,
    ports::{self, Port},
    speaker,
    sysinfo::SystemInfo,
    vga::{buffers, cursor},
};
use core::{
    arch::asm,
    ffi::{CStr, c_char, c_void},
    hint,
    panic::PanicInfo,
    sync::atomic::{AtomicU64, Ordering},
};

/// Sets everything up for, then triggers a kernel panic.
///
/// Runs in four different modes, `badbug`, `exception`, `exception noerror`, `const`.
///
/// ### badbug
/// Indicates a general case panic due to a bad bug, such as what would usually cause
/// the `panic!` macro to be ran.
/// Takes a format string for the panic's info.
///
/// ### exception
/// Creates a handler function for exceptions, taking the cause of the error as a `&CStr`,
/// and a `fn(u64)` function pointer to print information about the error code.
///
/// ### exception noerror
/// Same as `exception` except without the error code and it's `fn(u64)` function pointer.
///
/// ### const
/// Const available panicking, taking an `&'static str` as it's cause.
#[macro_export]
macro_rules! PANIC {
    (exception $cause:expr, $info:expr) => {{
        #[allow(unused)]
        extern "x86-interrupt" fn wrapper(stackframe: $crate::interrupts::IntStackFrame, errcode: u64) -> ! {
            use $crate::panic::kpanic;
            use core::ffi::c_char;

            // The last test ran by tests::run_tests, checks that a stack overflow
            // causes a page fault, so we need to exit running tests in it's handler
            #[cfg(test)]
            {
                if $cause == c"PAGE FAULT" {
                    println!("test stack overflow causes #PF - passed");
                    println!("\nIt looks like you didn't break anything!");
                    $crate::tests::exit_qemu(false);
                }
            }

            static mut IP: u64 = 0;
            static mut ERRCODE: u64 = 0;

            extern "sysv64" fn info() {
                let errcode = $info;

                // Safety: The statics are only ever written to once
                unsafe {
                    let ip = IP;
                    println!("Instruction: 0x{ip}");
                    errcode(ERRCODE)
                }
            }

            let cause = $cause as *const _ as *const c_char;
            unsafe {
                IP = stackframe.ip;
                ERRCODE = errcode;
                kpanic(cause, stackframe.sp, info);
            }
        }

        wrapper as *const () as u64
    }};

    (exception noerror $cause:expr) => {{
        #[allow(unused_imports)]
        extern "x86-interrupt" fn wrapper(stackframe: $crate::interrupts::IntStackFrame) -> ! {
            use $crate::panic::kpanic;
            use core::ffi::c_char;

            static mut IP: u64 = 0;
            extern "sysv64" fn info() {
                // Safety: The static's only ever written to once
                unsafe { let ip = IP; println!("Instruction: 0x{ip}") }
            }

            let cause = $cause as *const _ as *const c_char;
            unsafe {
                IP = stackframe.ip;
                kpanic(cause, stackframe.sp, info);
            }
        }

        wrapper as *const () as u64
    }};

    (badbug $($fmt:tt)+) => {{
        #[allow(unused)] // may be called from unsafe code or with existing imports
        use core::{ffi::{c_void, c_char}, ptr::null, arch::asm, mem, fmt::Arguments};

        let rsp: *const c_void;
        unsafe { asm!("cli", "mov {0}, rsp", out(reg) rsp) };
        let fmt = format_args!($($fmt)+);

        unsafe {
            static mut CAUSE:  *const c_char = null();
            static mut ARGUMENTS: Arguments = format_args!("");

            // Safety: Since PANIC! never returns, anything local passed to it will live forever
            ARGUMENTS = mem::transmute::<Arguments<'_>, Arguments<'static>>(fmt);
            CAUSE = c"BADBUG".as_ptr();

            extern "sysv64" fn info() {
                // Safety: Only ever written to once above
                unsafe { let args = ARGUMENTS; println!("{args}") }
            }

            let cause = CAUSE; // copy out of static
            asm!(
                "mov rdi, {0}",
                "mov rsi, {1}",
                "mov rdx, {2}",
                "call kpanic", // must be a call to allow stack trace
                "jmp hang",
                in(reg) cause,
                in(reg) rsp,
                in(reg) info
             );

            ::core::hint::unreachable_unchecked()
        }
    }};

    (const $cause:expr) => {
        panic!($cause)
    }
}

/// Triggers a kernel panic.
/// # Safety
/// This function should only be called via the [`PANIC`] macro.
#[rustfmt::skip]
#[cfg_attr(test, allow(unused))]
#[unsafe(no_mangle)]
pub unsafe extern "sysv64" fn kpanic(
    cause: *const c_char,
    sp: *const c_void,
    info: extern "sysv64" fn(),
) -> ! {
    /// The total number of panics which have occurred,
    /// useful for debugging problems with [`PANIC`] & [`kpanic`].
    static PANICS: AtomicU64 = AtomicU64::new(0);
    speaker::stop(); // in case anything was playing, prevent it from playing forever
    motor::force_disable(); // in case it was on
    cursor::ALLOW_ROW_0.store(true, Ordering::Relaxed);
    // Safety: Whoever was using the buffer is long gone now
    unsafe { buffers::BUFFER_HELD.store(false) };

    // Safety: The caller must ensure that cause points to a valid c str
    let cause = unsafe { CStr::from_ptr(cause) };

    print!("=============================\n  KERNEL PANIC: ");
    match cause.to_str() { // remove ugly debug quotation marks if possible
        Ok(s) => println!("{s}\n"),
        Err(_) => println!("{cause:?}\n"),
    }

    // Print kernel & hardware sysinfo
    let sysinfo = SystemInfo::now();
    println!(
        "Kernel: {}{}{}{}{}{} {} {} {} {}",
        sysinfo.idt_init as u8,
        sysinfo.gdt_init as u8,
        sysinfo.pic_init as u8,
        sysinfo.pit_init as u8,
        sysinfo.kbd_init as u8,
        sysinfo.fdc_init as u8,
        sysinfo.time,
        sysinfo.debug as u8,
        PANICS.fetch_add(1, Ordering::Relaxed),
        sysinfo.sfk_version
    );
    print!(
        "Hardware: {} {} ",
        sysinfo.cpu_vendor,
        sysinfo.floppy_space.unwrap_or(&0)
    );
    match sysinfo.date { // print the date if we have it
        Ok(d) => println!("{d}\n"),
        Err(e) => println!("{}\n", e.state),
    };

    info();
    stack_trace(6);

    // Print the top few elements on the stack
    // Safety: PANIC should have (hopefully) sent through a valid SP
    let valof = |offset| unsafe { *((sp as *const u64).wrapping_add(offset)) };
    println!(
        "\nStack (SP=0x{sp:?}):\n  {:#18x}  {:#18x}  {:#18x}\n  {:#18x}  {:#18x}  {:#18x}",
        valof(0),
        valof(1),
        valof(2),
        valof(3),
        valof(4),
        valof(5),
    );

    #[cfg(test)] // tests fail by panicking, but we still want to print error info
    crate::tests::exit_qemu(true);
    

    // Loop waiting for kbd input
    print!("\nPress ESC to restart device");
    cursor::update_visual_pos();
    loop {
        const ESC_SCANCODE_SET1: u8 = 0x01;
        const ESC_SCANCODE_SET2: u8 = 0x76;
        // Safety: Port 0x60 is fine to read as it just contains the last scancode
        let scancode = unsafe { ports::readb(Port::PS2Data) };
        if scancode == ESC_SCANCODE_SET1 || scancode == ESC_SCANCODE_SET2 {
            interrupts::triple_fault()
        }
        hint::spin_loop() // can't halt because of cli
    }
}

/// Prints a stack trace at most `frames` stackframes up.
#[unsafe(no_mangle)]
#[inline(never)]
fn stack_trace(frames: u32) {
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Stackframe {
        next: *const Stackframe,
        rip: u64,
    }

    let mut stack: *const Stackframe;
    // Safety: RBP (should) always point to the last stackframe,
    // even after interrupt handlers have been fired
    unsafe { asm!("mov {0}, rbp", out(reg) stack) }

    println!("\nStack trace (BP=0x{stack:?}):");
    for idx in 0..frames {
        // Safety: See safety comment above
        let sf = unsafe { *stack };
        stack = sf.next;

        // bootloader nicely ends the stackframe list with a null for us
        if stack.is_null() {
            return;
        }

        if sf.rip != 0 {
            println!("  {idx}  {:#8x}", sf.rip)
        }
    }
}

/// Ran when the `panic!` macro is invoked.
#[panic_handler]
#[cfg_attr(test, allow(unused))]
fn panic_handler(panic_info: &PanicInfo) -> ! {
    #[cfg(test)]
    {
        // tests fail by panicking
        use crate::tests::exit_qemu;
        println!("- failed, see failure cause below\n{panic_info}");
        exit_qemu(true);
    }

    let location = panic_info.location().unwrap(); // always succeeds
    PANIC!(badbug "External panic at {}:{}:{}\n{}",
        location.file(),
        location.line(),
        location.column(),
        panic_info.message()
    )
}
