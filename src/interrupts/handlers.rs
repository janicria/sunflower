use super::idt::Idt;
use crate::{
    ports, speaker, state,
    vga::{self, Color},
};

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
    ($name: ident = $id: expr) => {{
        #[unsafe(naked)]
        extern "C" fn wrapper() -> ! {
            core::arch::naked_asm!(
                "mov rdi, rsp",
                concat!("call ", stringify!($name)),
                "mov al, PREV_EXP", // move prev to second last
                "mov SECOND_LAST_EXP, al",
                concat!("mov al, ", stringify!($id)), // move id to prev
                "mov PREV_EXP, al",
                "call ohnonotgood",
                "iretq"
            )
        }
        wrapper
    }};
}

/// Creates a wrapper function for the handler which requires an error code with name `name`.
macro_rules! err_code_handler_wrapper {
    ($name: ident = $id: expr) => {{
        #[unsafe(naked)]
        extern "C" fn wrapper() -> ! {
            core::arch::naked_asm!(
                "pop rsi",      // err code as second arg
                "mov rdi, rsp", // stack frame as first arg
                concat!("call ", stringify!($name)),
                "mov al, PREV_EXP", // move prev to second last
                "mov SECOND_LAST_EXP, al",
                concat!("mov al, ", stringify!($id)), // move id to prev
                "mov PREV_EXP, al",
                "call ohnonotgood",
                "iretq"
            )
        }
        wrapper
    }};
}

/// Creates a wrapper function which calls the handler with name `name` then sends the eoi command.
macro_rules! send_eoi_handler {
    ($eoi: expr, $name: ident) => {{
        #[unsafe(naked)]
        extern "C" fn wrapper() -> ! {
            core::arch::naked_asm!(
                concat!("call ", stringify!($name)),
                "push rdi",
                concat!("mov rdi, ", stringify!($eoi)),
                "call eoi", // send eoi command
                "pop rdi",
                "iretq",
            )
        }
        wrapper
    }};
}

/// Creates an extremely basic handler.
macro_rules! basic_handler {
    ($name: ident, $error: expr) => {
        #[unsafe(no_mangle)]
        extern "C" fn $name(frame: StackFrame) {
            vga::print_color(concat!("EXCEPTION OCCURRED: ", $error), Color::Red);
            println!("\n{frame}");
        }
    };
}

basic_handler!(divide_by_zero_handler, "Attempted to divide by 0");
basic_handler!(breakpoint_handler, "Breakpoint");
basic_handler!(invalid_op_handler, "Invalid opcode");
basic_handler!(double_fault_handler, "Double Fault");
basic_handler!(gpf_handler, "General protection fault");

/// Sets the handlers for the IDT.
pub(super) fn add_handlers(idt: Idt) -> Idt {
    idt.set_handler(0, handler_wrapper!(divide_by_zero_handler = 0))
        .set_handler(3, handler_wrapper!(breakpoint_handler = 3))
        .set_handler(6, handler_wrapper!(invalid_op_handler = 6))
        .set_handler(8, err_code_handler_wrapper!(double_fault_handler = 8))
        .set_handler(13, err_code_handler_wrapper!(gpf_handler = 13))
        .set_handler(14, err_code_handler_wrapper!(page_fault_handler = 14))
        .set_handler(32, send_eoi_handler!(32, timer_handler))
        .set_handler(33, send_eoi_handler!(33, key_pressed_handler))
}

/////////////////////////////////////////////////////////
// Handlers
/////////////////////////////////////////////////////////

#[unsafe(no_mangle)]
extern "C" fn page_fault_handler(frame: StackFrame, code: u64) {
    println!("EXCEPTION OCCURRED: Page fault");
    vga::print_color("====== PAGE FAULT INFO ======\n", Color::Yellow);
    println!(
        "Caused by: {present}
Access: {access}
Causer: {causer}
Reserved write: {rwrite}
Caused by instruction fetch: {fetch}
Caused by protection key violation: {pkey}
Caused by shadow stack access: {sstack}
Virtual Address: {addr}\n{frame}",
        present = crate::bit_eq!(code, 1, "Page-protection Violation", "Non-present page"),
        access = crate::bit_eq!(code, 2, "Write", "Read"),
        causer = crate::bit_eq!(code, 3, "User program", "Privileged program"),
        rwrite = crate::bit_eq!(code, 4, "Yes", "No"),
        fetch = crate::bit_eq!(code, 5, "Yes", "No"),
        pkey = crate::bit_eq!(code, 6, "Yes", "No"),
        sstack = crate::bit_eq!(code, 7, "Yes", "No"),
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
extern "C" fn timer_handler() {
    unsafe {
        state::TIME += 1;

        if speaker::REPEATING {
            // Quickly disable to current sound
            let sound = ports::readb(ports::Port::Speaker) ^ 0b0000_0011;
            ports::writeb(ports::Port::Speaker, sound)
        }
    }
}
