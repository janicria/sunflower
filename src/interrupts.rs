use crate::{
    time, vga,
    wrappers::{InitError, InitLater},
};
use core::{arch::asm, convert::Infallible, fmt::Display, hint};
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

static IDT: InitLater<Idt> = InitLater::uninit();

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
pub fn load_idt() -> Result<(), LoadIDTError> {
    let idt = Idt::new();
    // Safety: Using properly filled out IDT.
    let loaded_idt = unsafe { idt.load() };

    // Return Err if the load fails
    if let Err(e) = IDT.init(Idt::new()) {
        return Err(LoadIDTError::Load(e));
    }

    let mut stored_idt = IDTDescriptor::default();
    // Store loaded IDT into a local variable. Safety: We're just storing a value
    unsafe { asm!("sidt [{}]", in(reg) (&mut stored_idt), options(nostack)) };

    // Return Err if sidt (store IDT) != descriptor passed to lidt
    if stored_idt != loaded_idt {
        return Err(LoadIDTError::Store("Stored IDT doesn't match loaded IDT"));
    }

    Ok(())
}

/// The error returned from `load_idt`.
pub enum LoadIDTError {
    Load(InitError<Idt>),
    Store(&'static str),
}

impl Display for LoadIDTError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LoadIDTError::Load(e) => write!(f, "Failed loading IDT - {e}"),
            LoadIDTError::Store(e) => write!(f, "{e}"),
        }
    }
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

        // Update the cursor every 100 ms
        if time::get_time().is_multiple_of(10) {
            vga::update_vga_cursor();
        }
    }
}

/// Sets external interrupts.
pub fn sti() {
    unsafe { asm!("sti") }
}

/// Clears external interrupts.
pub fn cli() {
    unsafe { asm!("cli") }
}

/// Causes a triple fault.
/// Can be used as the stupidest way ever to restart the device.
fn triple_fault() {
    // Safety: We're deliberately being very unsafe here
    unsafe {
        Idt::invalid().load(); // nuke IDT
        asm!("int 99"); //  gpf -> double fault -> triple fault
    }
}
