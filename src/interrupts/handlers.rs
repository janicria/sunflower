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
            unsafe { core::arch::naked_asm!("mov rdi, rsp", concat!("call ", stringify!($name)),) }
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
        extern "C" fn $name(frame: StackFrame) -> ! {
            println!(concat!("EXCEPTION OCCURED: ", $error));
            println!("{frame}");
            loop {}
        }
    };
}

basic_handler!(divide_by_zero_handler2, "Attempted to divide by 0");

/// Sets the handlers for the IDT.
pub(super) fn add_handlers(idt: Idt) -> Idt {
    idt.set_handler(0, handler_wrapper!(divide_by_zero_handler2))
        .set_handler(14, handler_wrapper!(err_code page_fault_handler))
}

/// Checks if the bit-th last bit in val is set
macro_rules! bit_eq {
    ($val: expr, $bit: expr) => {
        $val == $val | 0b0000_0001 << $bit - 1
    };
}

#[unsafe(no_mangle)]
extern "C" fn divide_by_zero_handler(frame: StackFrame) -> ! {
    println!("EXCEPTION OCCURED: Attempted to divide by 0\n{frame}");
    loop {}
}

#[unsafe(no_mangle)]
extern "C" fn page_fault_handler(frame: StackFrame, code: u64) -> ! {
    let present = match bit_eq!(code, 1) {
        true => "Page-protection Violation",
        false => "Non-present page",
    };
    let causer = match bit_eq!(code, 3) {
        true => "User",
        false => "Supervisor",
    };

    println!("EXCEPTION OCCURED: Page fault\n====== PAGE FAULT INFO ======\nPresent Bit: {present}\nAccess: {access}
Causer: {causer}
Reserved write: {rwrite}
Caused by instruction fetch: {fetch}
Caused by protection key violation: {pkey}
Caused by shadow stack access: {sstack}
Virtual Address: {addr}\n{frame}",
        access = if bit_eq!(code, 2) { "Write" } else { "Read" },
        rwrite = bit_eq!(code, 4),
        fetch = bit_eq!(code, 5),
        pkey = bit_eq!(code, 6),
        sstack = bit_eq!(code, 7),
        addr = unsafe { x86::controlregs::cr2() },
    );
    loop {}
}
