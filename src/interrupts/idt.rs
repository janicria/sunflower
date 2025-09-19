use super::{
    IntStackFrame,
    rbod::{self, ErrCodeHandler, ErrorCode, RbodErrInfo},
};
use crate::vga::{self, Color};
use core::{
    arch::{asm, naked_asm},
    mem,
    sync::atomic::Ordering,
};

type Handler = u64;

/// Error code argument passed to cont and rbod.
#[unsafe(no_mangle)]
pub static mut ERR_CODE: ErrorCode = ErrorCode::Invalid;

/// The Interrupt Descriptor Table.
pub struct Idt([InterruptDescriptor; 256]);

/// The value used by the lidt instruction load the IDT.
#[derive(PartialEq, Default)]
#[repr(C, packed)]
pub struct IDTDescriptor {
    size: u16,
    offset: *const Idt,
}

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
    rbod::SMALL_ERRS.fetch_add(1, Ordering::Relaxed);
    vga::print_color("An unexpected error occurred: ", Color::LightRed);
    let err = unsafe { ERR_CODE };
    println!("{err:?} at {:x}", frame.ip);
}

/// Calls rbod, never returns.
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
    /// This function only creates an IDT, and doesn't load it.
    #[allow(clippy::fn_to_numeric_cast, clippy::identity_op)]
    pub fn new() -> Self {
        /// Where IRQ vectors start in the table.
        static IRQ_START: usize = 32;

        let mut idt = Idt([InterruptDescriptor::default(); 256]);

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
        idt.set_handler(IRQ_START + 0, timer_handler as Handler);
        idt.set_handler(IRQ_START + 1, key_pressed_wrapper as Handler);
        idt.set_handler(IRQ_START + 7, dummy_handler as Handler);
        idt.set_handler(IRQ_START + 8, rtc_handler as Handler);
        idt.set_handler(IRQ_START + 15, dummy_handler as Handler);

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

    /// Loads the table into the `IDTR` register.
    /// Returns the created `IDTDescriptor`.
    /// # Safety
    /// Very bad things will happen if `self` isn't properly filed out.
    pub unsafe fn load(&self) -> IDTDescriptor {
        let descriptor = IDTDescriptor {
            size: (size_of::<Idt>() - 1) as u16,
            offset: self,
        };

        unsafe {
            asm!("lidt ({0})", in(reg) &descriptor, options(att_syntax, nostack));
        }

        descriptor
    }
}

/// An entry in the `IDT`
#[derive(Debug, Clone, Copy, Default)]
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
}

/// Immediately returns.
extern "x86-interrupt" fn dummy_handler(_frame: IntStackFrame) {}

/// Returns `set` if the `bit`th bit in `code` is set, otherwise returns `clear`.
fn bit_set(code: u64, bit: u64, set: &'static str, clear: &'static str) -> &'static str {
    if code == code | 1 << bit { set } else { clear }
}

/// Ran when a page fault occurs.
extern "x86-interrupt" fn page_fault_handler(frame: IntStackFrame, err_code: u64) {
    super::rbod::rbod(
        ErrorCode::PageFault,
        RbodErrInfo::Exception(frame),
        ErrCodeHandler::new(handler, err_code),
    );

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
extern "x86-interrupt" fn gpf_handler(frame: IntStackFrame, err_code: u64) {
    super::rbod::rbod(
        ErrorCode::GeneralProtectionFault,
        RbodErrInfo::Exception(frame),
        ErrCodeHandler::new(handler, err_code),
    );

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
extern "x86-interrupt" fn double_fault_handler(frame: IntStackFrame, _err_code: u64) {
    super::rbod::rbod(ErrorCode::DoubleFault, RbodErrInfo::Exception(frame), None)
}

/// Ran when the PIT generates an interrupt.
#[unsafe(naked)]
extern "C" fn timer_handler() -> ! {
    naked_asm!(
        pushregs!(),
        "lock inc qword ptr [TIME]", // increase time
        "mov rdi, 32",               // timer eoi
        "call eoi",
        popregs!(),
        "iretq",
    );
}

/// Ran when the PS/2 keyboard generates an interrupt.
#[unsafe(naked)]
extern "C" fn key_pressed_wrapper() -> ! {
    naked_asm!(
        pushregs!(),
        "call kbd_handler", // Safety: it's safe to read from port 0x60 in the key pressed interrupt
        "mov rdi, 33",      // key pressed eoi
        "call eoi",         // send eoi command
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
        "mov rdi, 40", // RTC eoi
        "call eoi",    // send eoi cmd
        popregs!(),    // restore regs
        "pop ax",
        "pop dx",
        "iretq" // return from int
    )
}
