use super::IDT;
use super::idt::Idt;
use crate::interrupts::keyboard;
use crate::ports;

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
            #[allow(unused_unsafe)]
            unsafe {
                core::arch::naked_asm!("mov rdi, rsp", concat!("call ", stringify!($name)), "iretq")
            }
        }
        wrapper
    }};
}

/// Creates a wrapper function for the handler which requires an error code with name `name`.
macro_rules! err_code_handler_wrapper {
    (err_code $name: ident) => {{
        #[unsafe(naked)]
        extern "C" fn wrapper() -> ! {
            #[allow(unused_unsafe)]
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

/// Creates a wrapper function which calls the handler with name `name` then sends the eoi command.
macro_rules! send_eoi_handler {
    ($eoi: expr, $name: ident) => {{
        #[unsafe(naked)]
        extern "C" fn wrapper() -> ! {
            #[allow(unused_unsafe)]
            unsafe {
                core::arch::naked_asm!(
                    "mov rdi, rsp", // stack frame
                    concat!("call ", stringify!($name)),
                    concat!("mov rdi, ", stringify!($eoi)),
                    "call eoi", // send eoi command
                    "iretq",
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
        .set_handler(8, err_code_handler_wrapper!(err_code double_fault_handler))
        .set_handler(13, err_code_handler_wrapper!(err_code gpf_handler))
        .set_handler(14, err_code_handler_wrapper!(err_code page_fault_handler))
        .set_handler(32, send_eoi_handler!(32, timer_handler))
        .set_handler(33, send_eoi_handler!(33, key_pressed_handler))
        .set_handler(255, handler_wrapper!(cause_triple_fault))
}

/////////////////////////////////////////////////////////
// Handlers
/////////////////////////////////////////////////////////

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
        causer = bit_eq!(code, 3, "User program", "Privileged program"),
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
// todo: print error code
extern "C" fn gpf_handler(frame: StackFrame, _code: u64) -> ! {
    println!("EXCEPTION OCCURRED: General protection fault\n{frame}");
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

#[unsafe(no_mangle)]
extern "C" fn timer_handler(_frame: StackFrame) {}

#[unsafe(no_mangle)]
extern "C" fn key_pressed_handler(_frame: StackFrame) {
    let key = ports::readb(ports::Port::PS2Data);
    keyboard::print_key(key);
}
