use super::{
    Idt, IntStackFrame,
    rbod::{self, ErrCodeHandler, ErrorCode, RbodErrInfo},
};
use crate::{gdt, vga::buffers, wrappers::TableDescriptor};
#[cfg(test)]
use crate::{interrupts::IDT, tests::exit_qemu};
use core::{
    arch::{asm, naked_asm},
    sync::atomic::Ordering,
};

type Handler = u64;

/// Where IRQ vectors start in the IDT.
static IRQ_START: usize = 32;

/// Error code argument passed to cont and rbod.
#[unsafe(no_mangle)]
pub static mut ERR_CODE: ErrorCode = ErrorCode::Invalid;

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
    print!(fg = LightRed, "An unexpected error occurred: ");
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
        let mut idt = Idt([InterruptDescriptor::default(); 256]);

        // A list of entry IDs can be found at: https://wiki.osdev.org/Exceptions
        idt.set_handler(0, None, rbod_wrapper!(0));
        idt.set_handler(1, None, rbod_wrapper!(1));
        idt.set_handler(2, None, rbod_wrapper!(2));
        idt.set_handler(3, None, cont_wrapper!(3, 0));
        idt.set_handler(5, None, rbod_wrapper!(5));
        idt.set_handler(6, None, cont_wrapper!(6, 2));
        idt.set_handler(7, None, rbod_wrapper!(7));
        idt.set_handler(8, Some(1), double_fault_handler as Handler);
        idt.set_handler(13, None, gpf_handler as Handler);
        idt.set_handler(14, None, page_fault_handler as Handler);
        idt.set_handler(IRQ_START + 0, None, timer_handler as Handler);
        idt.set_handler(IRQ_START + 1, None, key_pressed_wrapper as Handler);
        idt.set_handler(IRQ_START + 7, None, dummy_handler as Handler);
        idt.set_handler(IRQ_START + 8, None, rtc_handler as Handler);
        idt.set_handler(IRQ_START + 15, None, dummy_handler as Handler);

        idt
    }

    /// Returns an invalid IDT.
    pub const fn invalid() -> Self {
        Idt([InterruptDescriptor::invalid(); 256])
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

        unsafe {
            asm!("lidt ({0})", in(reg) &descriptor, options(att_syntax, nostack));
        }

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
    /// Creates a new, empty descriptor.
    const fn invalid() -> Self {
        InterruptDescriptor {
            offset_low: 0,
            selector: 0,
            ist: 0,
            attributes: 0,
            offset_middle: 0,
            offset_high: 0,
            _reserved: 0,
        }
    }

    /// Returns a new descriptor using `handler` as it's offset and `ist` for the IST.
    fn new(offset_ptr: Handler, ist: u8) -> Self {
        /// Present = 1, dpl = 0, must be zero = 0, gate type = interrupt,
        static FLAGS: u8 = 0b1_00_0_1111;

        // Force the ist to be only 3 bits, as remaining bits are reserved
        if ist > 0b111 {
            warn!("attempted creating an int descriptor with an ist > 7, which will be truncated!");
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

/// Immediately returns.
#[inline(never)]
extern "x86-interrupt" fn dummy_handler(_frame: IntStackFrame) {}

/// Returns `set` if the `bit`th bit in `code` is set, otherwise returns `clear`.
fn bit_set(code: u64, bit: u64, set: &'static str, clear: &'static str) -> &'static str {
    if code == code | 1 << bit { set } else { clear }
}

/// Ran when a page fault occurs.
#[inline(never)]
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
            "  Cause: {present}  Address: {addr}  Privilege: {causer}\n  Flags: {rwrite}{instruction}{pkey}{sstack}\n"
        )
    }
}

/// Ran when a general protection fault occurs.
#[inline(never)]
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
#[unsafe(naked)]
#[unsafe(no_mangle)]
extern "C" fn double_fault_handler() -> ! {
    naked_asm!(
        "cli",                         // just in case ints got enabled somehow
        "pop rax",                     // remove the empty error code double faults push
        "mov rdi, rsp",                // store stack frame in first arg
        "call print_df_info",          // print error info
        "mov rax, 0xDFDFDFDFDFDFDFDF", // pseudo error message which can be viewed in QEMU
        "call hang",                   // no turning back now
    );
}

/// Used by the double fault handler to print an error message.
#[unsafe(no_mangle)]
#[allow(unused)]
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that various interrupt descriptors point to their respective handlers.
    #[test_case]
    fn descriptors_point_to_handlers() {
        let idt = IDT.read().unwrap().0;
        assert_eq!(idt[8].ptr(), double_fault_handler as Handler);
        assert_eq!(idt[13].ptr(), gpf_handler as Handler);
        assert_eq!(idt[14].ptr(), page_fault_handler as Handler);
        assert_eq!(idt[IRQ_START + 0].ptr(), timer_handler as Handler);
        assert_eq!(idt[IRQ_START + 1].ptr(), key_pressed_wrapper as Handler);
        assert_eq!(idt[IRQ_START + 7].ptr(), dummy_handler as Handler);
        assert_eq!(idt[IRQ_START + 8].ptr(), rtc_handler as Handler);
        assert_eq!(idt[IRQ_START + 15].ptr(), dummy_handler as Handler);
    }
}
