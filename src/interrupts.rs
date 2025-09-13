use core::{arch::asm, convert::Infallible, hint, mem};
use idt::{IDTDescriptor, Idt};
use keyboard::KbdInitError;

/// IDT and exception handlers.
mod idt;

/// Basic PS/2 keyboard input detector.
mod keyboard;

/// Loads PIC and allows sending EOI.
mod pic;

/// Handles exceptions and panics.
mod rbod;

static mut IDT: Idt = Idt::invalid();

/// The interrupt stack frame.
#[derive(Debug, Default)]
#[repr(C)]
struct IntStackFrame {
    ip: u64,
    cs: u64,
    flags: u64,
    sp: u64,
    ss: u64,
}

/// Loads the IDT.
pub fn load_idt() -> Result<(), &'static str> {
    /// IDT Descriptor stored from sidt.
    #[unsafe(no_mangle)]
    static mut STORED_IDT: IDTDescriptor = unsafe { mem::zeroed() };

    unsafe {
        // Load idt
        let idt = Idt::new();
        let loaded_idt = idt.load();
        IDT = Idt::new();

        // Return Err if sidt (store IDT) != descriptor passed to lidt
        asm!("sidt [STORED_IDT]", options(nostack));
        if STORED_IDT != loaded_idt {
            return Err("Stored IDT doesn't match loaded IDT");
        }
    }

    Ok(())
}

/// Initialises the PIC.
pub fn init_pic() -> Result<(), Infallible> {
    pic::init();
    Ok(())
}

/// Initialises the PS/2 keyboard.
pub fn init_kbd() -> Result<(), KbdInitError> {
    keyboard::init()
}

/// Repeatedly loops polling the keyboard.
pub fn kbd_poll_loop() -> ! {
    loop {
        hint::spin_loop(); // pause instruction
        keyboard::poll_keyboard();
    }
}

/// Sets external interrupts.
fn sti() {
    unsafe { asm!("sti") }
}

/// Clears external interrupts.
fn cli() {
    unsafe { asm!("cli") }
}

/// Causes a triple fault.
/// Can be used as the stupidest way ever to restart the device.
fn triple_fault() {
    unsafe {
        Idt::invalid().load(); // nuke IDT
        asm!("int 99"); //  gpf -> double fault -> triple fault
    }
}
