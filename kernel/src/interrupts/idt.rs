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
    kernel/src/interrupts/idt.rs

    Handles loading the IDT and it's handlers.
    Contained within the interrupts module
*/

use super::{IRQ_START, Idt, IntStackFrame};
use crate::{gdt, vga::buffers};
#[cfg(test)]
use crate::{interrupts::IDT, tests::exit_qemu};
use core::arch::{asm, naked_asm};
use libutil::TableDescriptor;

type Handler = u64;

/// Pushes all registers which need to be saved before calling C ABI functions.
macro_rules! pushregs {
    () => {
        "push rdi
        push rax
        push rcx
        push rdx
        push rsi
        push r8
        push r9
        push r10
        push r11"
    };
}

/// Pops all registers which need to be saved before calling C ABI functions.
macro_rules! popregs {
    () => {
        "pop r11
        pop r10
        pop r9
        pop r8
        pop rsi
        pop rdx
        pop rcx
        pop rax
        pop rdi"
    };
}

/// Calls cont, increases the return address, then returns from the interrupt.
macro_rules! cont_wrapper {
    ($err: expr, $inc: expr) => {{
        #[unsafe(naked)]
        extern "C" fn wrapper() -> ! {
            naked_asm!(
                pushregs!(),                            // save state before calling cont
                concat!("mov word ptr ERR_CODE, ", stringify!($err)), // err code
                "mov rdi, rsp",                         // store stack frame in first arg
                "add rdi, 9*8",                         // offset the 9 registers just got pushed
                "call cont",
                popregs!(),                             // restore state now that cont has finished
                concat!("add qword ptr [rsp], ", $inc), // increase return address to not get in an infinite cycle
                "iretq"
            )
        }

        wrapper as *const () as Handler
    }};
}

/// Prints the error passed by the wrapper.
#[unsafe(no_mangle)]
#[cfg_attr(test, allow(unused_variables))]
extern "C" fn cont(frame: IntStackFrame) {
    #[unsafe(no_mangle)]
    static mut ERR_CODE: ErrCode = ErrCode::Invalid;

    #[derive(Debug, Clone, Copy)]
    #[repr(u8)]
    #[allow(unused)]
    enum ErrCode {
        Breakpoint = 3,
        InvalidOpcode = 6,
        Invalid = 255,
    }

    // some tests deliberately trigger cont exceptions
    #[cfg(not(test))]
    {
        // Safety: ERR_CODE is only ever accessed here and in cont_wrapper
        let error = unsafe { ERR_CODE };
        println!(
            fg = LightRed,
            "An unexpected error occurred: {error:?} at 0x{:x}", frame.ip
        );
    }
}

/// Triggers a kernel panic, never returns.
macro_rules! panic_wrapper {
    ($err: expr) => {{
        extern "x86-interrupt" fn wrapper(frame: IntStackFrame) {
            panic!("{}, {frame}", $err)
        }

        wrapper as *const () as Handler
    }};
}

impl Idt {
    /// Creates a new, loaded table, with all it's required entries set.
    /// This function only creates an IDT, and doesn't load it.
    #[allow(clippy::fn_to_numeric_cast)]
    pub fn new() -> Self {
        let mut idt = Idt([InterruptDescriptor::default(); 256]);

        // A list of entry IDs can be found at: https://wiki.osdev.org/Exceptions
        idt.set_handler(0, None, panic_wrapper!(0));
        idt.set_handler(1, None, panic_wrapper!(1));
        idt.set_handler(2, None, panic_wrapper!(2));
        idt.set_handler(3, None, cont_wrapper!(3, 0));
        idt.set_handler(5, None, panic_wrapper!(5));
        idt.set_handler(6, None, cont_wrapper!(6, 2));
        idt.set_handler(7, None, panic_wrapper!(7));
        idt.set_handler(8, Some(1), double_fault_handler as *const () as Handler);
        idt.set_handler(13, None, gpf_handler as *const () as Handler);
        idt.set_handler(14, None, page_fault_handler as *const () as Handler);
        idt.set_handler(IRQ_START + 0, None, timer_handler as *const () as Handler);
        idt.set_handler(IRQ_START + 1, None, kbd_wrapper as *const () as Handler);
        idt.set_handler(IRQ_START + 6, None, floppy_handler as *const () as Handler);
        idt.set_handler(IRQ_START + 7, None, dummy_handler as *const () as Handler);
        idt.set_handler(IRQ_START + 8, None, rtc_handler as *const () as Handler);
        idt.set_handler(IRQ_START + 15, None, dummy_handler as *const () as Handler);

        idt
    }

    /// Sets the table's entry with id `entry_id`
    fn set_handler(&mut self, entry_id: usize, ist: Option<u8>, handler: Handler) {
        self.0[entry_id] = InterruptDescriptor::new(handler, ist.unwrap_or_default())
    }

    /// Loads the table into the `IDTR` register.
    /// Returns the created `IDTDescriptor`.
    /// # Safety
    /// Very bad things will happen if `self` isn't properly filed out.
    pub unsafe fn load(&'static self) -> TableDescriptor<Idt> {
        let descriptor = TableDescriptor::new(self);

        // Safety: The caller must ensure that the IDT is valid
        unsafe { asm!("lidt ({0})", in(reg) &descriptor, options(att_syntax, nostack)) }

        descriptor
    }
}

/// An entry in the `IDT`
/// [`Reference`](https://wiki.osdev.org/Interrupt_Descriptor_Table#Gate_Descriptor_2)
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct InterruptDescriptor {
    /// Offset bits 0..15
    offset_low: u16,

    /// The segment selector in the GDT
    selector: u16,

    /// The offset in the IST
    ist: u8,

    /// The gate type, dpl, and present bits
    attributes: u8,

    /// Offset bits 16..31
    offset_middle: u16,

    /// Offset bits 32..63
    offset_high: u32,
    _reserved: u32,
}

impl InterruptDescriptor {
    /// Returns a new descriptor using `handler` as it's offset and `ist` for the IST.
    fn new(offset_ptr: Handler, ist: u8) -> Self {
        /// Present = 1, dpl = 0, must be zero = 0, gate type = interrupt,
        static FLAGS: u8 = 0b1_00_0_1111;

        // Force the ist to be only 3 bits, as remaining bits are reserved
        if ist > 0b111 {
            warn!(
                "attempted creating an int descriptor with an ist > 7 ({ist}), which will be truncated!"
            );
        }
        let ist = ist & 0b111;

        InterruptDescriptor {
            selector: gdt::cs_register(),
            offset_low: offset_ptr as u16,
            offset_middle: (offset_ptr >> 16) as u16,
            offset_high: (offset_ptr >> 32) as u32,
            ist,
            attributes: FLAGS,
            _reserved: 0,
        }
    }

    /// Returns the descriptor's pointer / offset.
    #[cfg(test)]
    fn ptr(&self) -> Handler {
        let mut ptr = self.offset_low as u64;
        ptr |= (self.offset_middle as u64) << 16;
        ptr |= (self.offset_high as u64) << 32;
        ptr
    }
}

/// Immediately returns as a really terrible way of handling spurious IRQs.
/// Since IRQs 7 & 15 aren't used by sunflower anyways though, it's not that bad.
#[inline(never)]
extern "x86-interrupt" fn dummy_handler(_frame: IntStackFrame) {}

/// Returns `set` if the `bit`th bit in `code` is set, otherwise returns `clear`.
fn bit_set(code: u64, bit: u64, set: &'static str, clear: &'static str) -> &'static str {
    if code == code | 1 << bit { set } else { clear }
}

/// Ran when a page fault occurs.
#[inline(never)]
extern "x86-interrupt" fn page_fault_handler(frame: IntStackFrame, err_code: u64) {
    let present = bit_set(err_code, 0, "Page-protection Violation", "Non-present page");
    let causer = bit_set(err_code, 2, "User", "Privileged");
    let addr: usize;
    unsafe { asm!("mov {}, cr2", out(reg) addr) }

    let rwrite = bit_set(err_code, 3, "Reserved write, ", "");
    let instruction = bit_set(err_code, 4, "Instruction fetch, ", "");
    let pkey = bit_set(err_code, 5, "Protection key, ", "");
    let sstack = bit_set(err_code, 6, "Shadow stack", "");

    panic!(
        "PAGE FAULT
      {frame}
      Cause: {present} at address 0x{addr:x} by {causer}\n      Flags: {rwrite}{instruction}{pkey}{sstack}"
    );
}

/// Ran when a general protection fault occurs.
#[inline(never)]
extern "x86-interrupt" fn gpf_handler(frame: IntStackFrame, err_code: u64) {
    if err_code == 0 {
        panic!("General protection fault\n      {frame}\n      Not segment related");
    } else {
        // Reference: https://wiki.osdev.org/Exceptions#Selector_Error_Code
        let external = bit_set(err_code, 0, "True", "False");
        let idx = err_code >> 3;
        let table = (err_code >> 1) & 0b11;

        let descriptor = match table {
            0b00 => "GDT",
            0b01 | 0b10 => "IDT",
            0b11 => "LDT",
            _ => "Unknown", // this should never happen
        };

        panic!(
            "General protection fault\n      {frame}\n      External: {external} | Descriptor: {descriptor} | Selector index: {idx}"
        );
    }
}

/// Ran when a double fault occurs.
#[unsafe(naked)]
#[unsafe(no_mangle)]
extern "C" fn double_fault_handler() -> ! {
    naked_asm!(
        "cli",                         // just in case ints got enabled somehow
        "pop rax",                     // remove the empty error code double faults push
        "mov rdi, rsp",                // store stack frame in first arg
        "call print_df_info",          // print error info
        "mov rax, 0xDFDFDFDFDFDFDFDF", // pseudo error message which can be viewed in QEMU
        "jmp hang",                    // no turning back now
    );
}

/// Used by the double fault handler to print an error message.
#[unsafe(no_mangle)]
#[cfg_attr(test, allow(unused))]
extern "C" fn print_df_info(frame: IntStackFrame) {
    // The last test ran by tests::run_tests, checks that a stack overflow
    // causes a double fault, so we need to exit running tests in it's handler
    #[cfg(test)]
    {
        use core::any::type_name_of_val;
        println!("test {} - passed", type_name_of_val(&double_fault_handler));
        println!("\nIt looks like you didn't break anything!");
        exit_qemu(false);
    }

    // Safety: Whoever was holding that buffer is not going to be returned to anytime soon
    unsafe { buffers::BUFFER_HELD.store(false) }
    buffers::clear();

    println!(
        "Whoops... looks like a double fault!\n\nHere's some info about it:\n{frame}\n
Since double faults are pretty nasty, sunflower can't trust any kernel services to get keyboard input or wait, so you'll have to restart your device manually"
    );
}

/// Ran when the PIT generates an interrupt.
#[unsafe(naked)]
extern "C" fn timer_handler() -> ! {
    naked_asm!(
        pushregs!(),
        "lock inc qword ptr [TIME]",  // increase time
        "call dec_floppy_motor_time", // in floppy.rs
        "mov rdi, 0",                 // timer IRQ as first argument
        "call eoi",
        popregs!(),
        "iretq",
    );
}

/// Ran when the PS/2 keyboard generates an interrupt.
#[unsafe(naked)]
extern "C" fn kbd_wrapper() -> ! {
    naked_asm!(
        pushregs!(),
        "call kbd_handler", // Safety: it's safe to read from port 0x60 in the key pressed interrupt
        "mov rdi, 1",       // key pressed IRQ as first argument
        "call eoi",         // send eoi command
        popregs!(),
        "iretq",
    );
}

/// Ran when the floppy IRQ occurs.
#[unsafe(naked)]
extern "C" fn floppy_handler() -> ! {
    naked_asm!(
        pushregs!(),
        "mov rdi, 6", // floppy IRQ as first argument
        "call eoi",
        popregs!(),
        "iretq",
    );
}

/// Flag set by the RTC handler when the RTC finishes updating.
#[unsafe(no_mangle)]
static mut RTC_UPDATE_ENDED: u8 = 0;

/// Ran when the RTC generates an interrupt
#[unsafe(naked)]
extern "C" fn rtc_handler() -> ! {
    naked_asm!(
        "push dx", // backup regs
        "push ax",
        pushregs!(),
        "cmp byte ptr [RTC_UPDATE_ENDED], 1", // check if the update ended int has been sent
        "je rtc_ret",                         // if so, cancel all future interrupts
        "mov dx, 0x70",                       // cmos register selector
        "mov al, 0x8C",                       // select register C
        "out dx, al",                         // store register C as the next reg
        "mov dx, 0x71",                       // select select register C
        "in al, dx",                          // load register C into al
        "mov ah, al",                         // copy register C into ah
        "or ah, 16",                          // set bit 4
        "cmp al, ah",                         // if they're the same, bit 4 is set
        "je update_ended",                    // if so, set the RTC_UPDATE_ENDED flag
        "jmp rtc_ret"                         // if not return from the interrupt
    );
}

/// Ran when the RTC sends an update ended interrupt.
#[unsafe(naked)]
#[unsafe(no_mangle)]
extern "C" fn update_ended() {
    naked_asm!(
        "mov byte ptr [RTC_UPDATE_ENDED], 1", // set update ended flag to disable future interrupts
        "call sync_time_to_rtc",              // in time.rs
        "jmp rtc_ret"                         // return from interrupt
    )
}

/// Returns from the RTC handler.
#[unsafe(naked)]
#[unsafe(no_mangle)]
extern "C" fn rtc_ret() {
    naked_asm!(
        "mov rdi, 8", // RTC IRQ as first argument
        "call eoi",   // send eoi cmd
        popregs!(),   // restore regs
        "pop ax",
        "pop dx",
        "iretq" // return from int
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that various interrupt descriptors point to their respective handlers.
    #[test_case]
    #[rustfmt::skip]
    fn descriptors_point_to_handlers() {
        let idt = IDT.read().unwrap().0;
        assert_eq!(idt[8].ptr(),              double_fault_handler as *const () as Handler);
        assert_eq!(idt[13].ptr(),             gpf_handler          as *const () as Handler);
        assert_eq!(idt[14].ptr(),             page_fault_handler   as *const () as Handler);
        assert_eq!(idt[IRQ_START + 0].ptr(),  timer_handler   as *const () as Handler);
        assert_eq!(idt[IRQ_START + 1].ptr(),  kbd_wrapper     as *const () as Handler);
        assert_eq!(idt[IRQ_START + 6].ptr(),  floppy_handler  as *const () as Handler);
        assert_eq!(idt[IRQ_START + 7].ptr(),  dummy_handler   as *const () as Handler);
        assert_eq!(idt[IRQ_START + 8].ptr(),  rtc_handler     as *const () as Handler);
        assert_eq!(idt[IRQ_START + 15].ptr(), dummy_handler   as *const () as Handler);
    }

    /// Tests that [`cont_wrapper!`] handlers actually continue
    #[test_case]
    fn cont_handlers_continue() {
        // int3 = breakpoint, ud2 = UD
        unsafe { core::arch::asm!("int3", "ud2") }
    }
}
