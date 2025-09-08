use super::{
    IntStackFrame,
    rbod::{self, ErrCodeHandler, ErrorCode, RbodErrInfo},
};
use crate::vga::{self, Color};
use core::{
    arch::{asm, naked_asm},
    mem,
};

type Handler = u64;

/// Error code argument passed to cont and rbod.
#[unsafe(no_mangle)]
pub static mut ERR_CODE: ErrorCode = ErrorCode::Invalid;

/// The Interrupt Descriptor Table
pub struct Idt([InterruptDescriptor; 256]);

/// Pushes all registers with need to be saved before called C ABI functions.
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

/// Pops all registers with need to be saved before called C ABI functions.
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
                pushregs!(),                            // need to push before calling cont
                concat!("mov rdi, ", stringify!($err)), // err code
                "mov ERR_CODE, rdi",                    // store err code in static
                "mov rdi, rsp",                         // store stack frame in first arg
                "add rdi, 9*8",                         // offset the 9 registers just got pushed
                "call cont",
                popregs!(),
                concat!("add qword ptr [rsp], ", $inc), // increase return address to not get in an infinite cycle
                "iretq"
            )
        }

        wrapper as Handler
    }};
}

/// Continues execution after an error occurs.
#[unsafe(no_mangle)]
fn cont(frame: IntStackFrame) {
    unsafe { rbod::SMALL_ERRS += 1 };
    vga::print_color("An unexpected error occurred: ", Color::LightRed);
    let err = unsafe { ERR_CODE };
    println!("{err:?} at {:x}", frame.ip);
}

/// Calls rbod
macro_rules! rbod_wrapper {
    ($err: expr) => {{
        #[unsafe(naked)]
        extern "C" fn wrapper() -> ! {
            naked_asm!(
                concat!("mov rdi, ", stringify!($err)), // err code
                "mov ERR_CODE, rdi",                    // store err code in static
                "mov rdi, rsp",                         // store stack frame in first arg
                "call setup_rbod",                      // never returns so need for iretq
            )
        }

        wrapper as Handler
    }};
}

impl Idt {
    /// Creates a new, loaded table, with all it's required entries set.
    #[allow(clippy::fn_to_numeric_cast)]
    pub fn new() -> Self {
        let mut idt = Idt([InterruptDescriptor::empty(); 256]);
        // A list of entry IDs can be found at: https://wiki.osdev.org/Exceptions

        idt.set_handler(0, rbod_wrapper!(0));
        idt.set_handler(1, rbod_wrapper!(1));
        idt.set_handler(2, rbod_wrapper!(2));
        idt.set_handler(3, cont_wrapper!(3, 0));
        idt.set_handler(5, rbod_wrapper!(5));
        idt.set_handler(6, cont_wrapper!(6, 2));
        idt.set_handler(7, rbod_wrapper!(7));
        idt.set_handler(8, double_fault_handler as Handler);
        idt.set_handler(13, gpf_handler as Handler);
        idt.set_handler(14, page_fault_handler as Handler);
        idt.set_handler(32, timer_handler as Handler);
        idt.set_handler(33, key_pressed_wrapper as Handler);

        unsafe { idt.load() }
        idt
    }

    /// Returns an invalid IDT.
    pub const fn invalid() -> Self {
        unsafe { mem::transmute::<[u128; 256], Idt>([0u128; 256]) }
    }

    /// Sets the table's entry with id `entry_id`
    fn set_handler(&mut self, entry_id: usize, handler: Handler) {
        self.0[entry_id] = InterruptDescriptor::new(handler)
    }

    /// Loads the table into the IDTR register.
    pub unsafe fn load(&self) {
        /// The value used below for the lidt instruction to load the IDT.
        #[repr(C, packed)]
        struct IDTDescriptor {
            size: u16,
            offset: *const Idt,
        }

        let descriptor = IDTDescriptor {
            size: (size_of::<Self>() - 1) as u16,
            offset: self,
        };

        unsafe {
            asm!("lidt ({0})", in(reg) &descriptor, options(att_syntax));
        }

        vga::print_done("Loaded IDT");
    }
}

/// An entry in the `IDT`
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct InterruptDescriptor {
    offset_low: u16,    // offset bits 0..15
    selector: u16,      // segment selector in the gdt
    ist: u8,            // ist offset
    attributes: u8,     // gate type, dpl, and present
    offset_middle: u16, // offset bits 16..31
    offset_high: u32,   // offset bits 32..63
    reserved: u32,
}

impl InterruptDescriptor {
    /// Returns a new descriptor using `handler` as it's offset.
    fn new(offset_ptr: Handler) -> Self {
        let cs_reg;
        unsafe { asm!("mov {0:x}, cs", out(reg) cs_reg) }

        InterruptDescriptor {
            selector: cs_reg,
            offset_low: offset_ptr as u16,
            offset_middle: (offset_ptr >> 16) as u16,
            offset_high: (offset_ptr >> 32) as u32,
            ist: 0,
            attributes: 0x8E, // gate type = interrupt, dpl = 0, present = 1
            reserved: 0,
        }
    }

    /// Returns a new descriptor an empty function as it's offset.
    #[allow(clippy::fn_to_numeric_cast)]
    fn empty() -> Self {
        extern "x86-interrupt" fn empty_handler(_frame: IntStackFrame) {}
        InterruptDescriptor::new(empty_handler as Handler)
    }
}

/// Returns `set` if the `bit`th bit in `code` is set, otherwise returns `clear`.
fn bit_set(code: u64, bit: u64, set: &'static str, clear: &'static str) -> &'static str {
    if code == code | 1 << bit { set } else { clear }
}

/// Ran when a page fault occurs.
unsafe extern "x86-interrupt" fn page_fault_handler(frame: IntStackFrame, err_code: u64) {
    unsafe {
        super::rbod::rbod(
            ErrorCode::PageFault,
            RbodErrInfo::Exception(frame),
            ErrCodeHandler::new(handler, err_code),
        )
    }

    fn handler(err_code: u64) {
        let present = bit_set(err_code, 0, "Page-protection Violation", "Non-present page");
        let causer = bit_set(err_code, 2, "User", "Privileged");
        let addr: usize;
        unsafe { asm!("mov {}, cr2", out(reg) addr) }

        let rwrite = bit_set(err_code, 3, "Reserved write, ", "");
        let instruction = bit_set(err_code, 4, "Instruction fetch, ", "");
        let pkey = bit_set(err_code, 5, "Protection key, ", "");
        let sstack = bit_set(err_code, 6, "Shadow stack", "");

        println!(
            "  Cause: {present}   Address: {addr}   Privilege: {causer}\n  Flags: {rwrite}{instruction}{pkey}{sstack}\n"
        )
    }
}

/// Ran when a general protection fault occurs.
unsafe extern "x86-interrupt" fn gpf_handler(frame: IntStackFrame, err_code: u64) {
    unsafe {
        super::rbod::rbod(
            ErrorCode::GeneralProtectionFault,
            RbodErrInfo::Exception(frame),
            ErrCodeHandler::new(handler, err_code),
        )
    };

    fn handler(err_code: u64) {
        if err_code == 0 {
            println!("  Not segment related\n\n")
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

            println!(
                "     Occurred externally: {external}   Descriptor: {descriptor}   Selector index: {idx}\n\n"
            );
        }
    }
}

/// Ran when a double fault occurs.
unsafe extern "x86-interrupt" fn double_fault_handler(frame: IntStackFrame, _err_code: u64) {
    unsafe { super::rbod::rbod(ErrorCode::DoubleFault, RbodErrInfo::Exception(frame), None) }
}

#[unsafe(naked)]
extern "C" fn timer_handler() -> ! {
    naked_asm!(
        pushregs!(),
        "mov rdi, TIME",
        "inc rdi", // increase time
        "mov TIME, rdi",
        "mov rdi, 32", // timer eoi
        "call eoi",
        popregs!(),
        "iretq",
    );
}

#[unsafe(naked)]
extern "C" fn key_pressed_wrapper() -> ! {
    naked_asm!(
        pushregs!(),
        "call key_pressed_handler", // in keyboard.rs
        "mov rdi, 33",              // key pressed eoi
        "call eoi",                 // send eoi command
        popregs!(),
        "iretq",
    );
}
