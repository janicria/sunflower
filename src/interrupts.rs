use idt::Idt;

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

/// Loads the IDT and initialises the PIC.
pub fn init() {
    unsafe { IDT = Idt::new() }
    pic::init()
}

/// Causes a triple fault.
/// Can be used as the stupidest way ever to restart the device.
fn triple_fault() {
    unsafe {
        Idt::invalid().load(); // nuke IDT
        core::intrinsics::unreachable() //  invalid op -> gpf -> double fault -> triple fault
    }
}
