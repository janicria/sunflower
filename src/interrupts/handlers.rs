use super::IDT;
use super::idt::Idt;

/// The stack frame right after an exception occurs.
#[derive(Debug)]
#[repr(C)]
struct StackFrame {
    instruction_ptr: u64,
    cs: u64,
    flags: u64,
    stack_ptr: u64,
    stack_segment: u64,
}

impl core::fmt::Display for StackFrame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "====== STACK FRAME ======\nInstruction pointer: {iptr}\nCode segment: {cs}\nCPU flags: {flags}\nStack pointer: {sptr}\nStack segment: {ss}",
            iptr = self.instruction_ptr,
            cs = self.cs,
            flags = self.flags,
            sptr = self.stack_ptr,
            ss = self.stack_segment
        )
    }
}

/// Creates a wrapper function for the handler with name `name`.
macro_rules! handler_wrapper {
    ($name: ident) => {{
        #[unsafe(naked)]
        extern "C" fn wrapper() -> ! {
            unsafe {
                core::arch::naked_asm!("mov rdi, rsp", concat!("call ", stringify!($name)), "iretq")
            }
        }
        wrapper
    }};
    (err_code $name: ident) => {{
        #[unsafe(naked)]
        extern "C" fn wrapper() -> ! {
            unsafe {
                core::arch::naked_asm!(
                    "pop rsi",      // err code
                    "mov rdi, rsp", // stack frame
                    concat!("call ", stringify!($name)),
                    "iretq"
                )
            }
        }
        wrapper
    }};
}

/// Creates an extremely basic handler.
macro_rules! basic_handler {
    ($name: ident, $error: expr) => {
        #[unsafe(no_mangle)]
        extern "C" fn $name(frame: StackFrame) {
            println!(concat!("EXCEPTION OCCURRED: ", $error));
            println!("{frame}");
        }
    };
}

basic_handler!(divide_by_zero_handler, "Attempted to divide by 0");
basic_handler!(breakpoint_handler, "Breakpoint");

/// Sets the handlers for the IDT.
pub(super) fn add_handlers(idt: Idt) -> Idt {
    idt.set_handler(0, handler_wrapper!(divide_by_zero_handler))
        .set_handler(3, handler_wrapper!(breakpoint_handler))
        .set_handler(8, handler_wrapper!(err_code double_fault_handler))
        .set_handler(14, handler_wrapper!(err_code page_fault_handler))
        .set_handler(255, handler_wrapper!(cause_triple_fault))
}

/// Equals `true` if the bit-th last bit in val is set
macro_rules! bit_eq {
    ($val: expr, $bit: expr, $true: expr, $false: expr) => {
        if $val == $val | 0b0000_0001 << $bit - 1 {
            $true
        } else {
            $false
        }
    };
}

#[unsafe(no_mangle)]
extern "C" fn page_fault_handler(frame: StackFrame, code: u64) {
    println!(
        "EXCEPTION OCCURRED: Page fault\n====== PAGE FAULT INFO ======
Caused by: {present}
Access: {access}
Causer: {causer}
Reserved write: {rwrite}
Caused by instruction fetch: {fetch}
Caused by protection key violation: {pkey}
Caused by shadow stack access: {sstack}
Virtual Address: {addr}\n{frame}",
        present = bit_eq!(code, 1, "Page-protection Violation", "Non-present page"),
        access = bit_eq!(code, 2, "Write", "Read"),
        causer = bit_eq!(code, 3, "User", "Supervisor"),
        rwrite = bit_eq!(code, 4, "Yes", "No"),
        fetch = bit_eq!(code, 5, "Yes", "No"),
        pkey = bit_eq!(code, 6, "Yes", "No"),
        sstack = bit_eq!(code, 7, "Yes", "No"),
        addr = load_cr2(),
    );
}

/// Returns the cr2 register.
fn load_cr2() -> usize {
    unsafe {
        let cr2;
        core::arch::asm!("mov {}, cr2", out(reg) cr2);
        cr2
    }
}

#[unsafe(no_mangle)]
extern "C" fn double_fault_handler(frame: StackFrame, _code: u64) -> ! {
    println!(
        "EXCEPTION OCCURRED: Double Fault\n{frame}\nDouble faults cannot return, entering idle state"
    );
    crate::idle()
}

#[unsafe(no_mangle)]
extern "C" fn cause_triple_fault(_frame: StackFrame) {
    println!("Causing deliberate triple fault!");
    unsafe {
        core::ptr::write(IDT.as_mut_ptr(), Idt::new()); // nuke IDT
        core::arch::asm!("ud2"); // cause double fault which escalates to a triple fault
        core::hint::unreachable_unchecked()
    }
}
