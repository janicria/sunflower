use crate::{
    time,
    vga::cursor,
    wrappers::{InitLater, LoadRegisterError, TableDescriptor},
};
use core::{arch::asm, convert::Infallible, fmt::Display, hint};
use idt::InterruptDescriptor;
use keyboard::KbdInitError;

/// IDT and exception handlers.
mod idt;

/// Basic PS/2 keyboard input detector.
mod keyboard;

/// Loads both PICs and allows sending EOI commands.
mod pic;

/// Handles exceptions and panics.
mod rbod;

/// Where IRQ vectors start in the IDT.
static IRQ_START: usize = 32;

/// The loaded IDT.
pub static IDT: InitLater<Idt> = InitLater::uninit();

/// The Interrupt Descriptor Table.
#[derive(Debug)]
#[repr(transparent)]
pub struct Idt([InterruptDescriptor; 256]);

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

impl Display for IntStackFrame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "  Location: {:x}   Flags: {}   Code segment: {}\n  Stack pointer: {:x}   Stack segment: {}",
            self.ip, self.flags, self.cs, self.sp, self.ss
        )
    }
}

/// Loads the IDT.
pub fn load_idt() -> Result<(), LoadRegisterError<Idt>> {
    let idt = IDT.init(Idt::new())?;
    dbg_info!("IDT loaded at 0x{:x}", idt as *const Idt as u64);

    // Safety: Using properly filled out IDT.
    let loaded_idt = unsafe { idt.load() };

    // Return Err if sidt (store IDT) != descriptor passed to lidt
    if idt_register() != loaded_idt {
        do yeet LoadRegisterError::Store("IDT");
    }

    Ok(())
}

/// Returns the current value in the GDT register.
pub fn idt_register() -> TableDescriptor<Idt> {
    let mut idt = TableDescriptor::invalid();
    // Safety: We're just storing a value
    unsafe { asm!("sidt [{}]", in(reg) (&mut idt), options(preserves_flags, nostack)) };
    idt
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
            cursor::update_visual_pos();
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
        let descriptor = TableDescriptor::<Idt>::invalid();
        asm!("lidt ({0})", in(reg) &descriptor, options(att_syntax)); // load invalid descriptor
        asm!("int 0x42") //  gpf -> double fault -> triple fault
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that various structs passed to the CPU are the size that the CPU expects them.
    #[test_case]
    fn structs_have_the_right_size() {
        assert_eq!(size_of::<IntStackFrame>(), 40);
        assert_eq!(size_of::<InterruptDescriptor>(), 16);
        assert_eq!(size_of::<Idt>(), size_of::<InterruptDescriptor>() * 256);
    }
}
