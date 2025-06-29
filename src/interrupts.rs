use idt::Idt;
use spin::Lazy;

mod handlers;
mod idt;
mod keyboard;
mod pics;

static IDT: Lazy<Idt> = Lazy::new(|| handlers::add_handlers(Idt::new()));

/// Loads the IDT and initialises the PIC.
pub fn init() {
    IDT.load();
    pics::init();
}

/// Causes a triple fault.
/// Can be used as the stupidest way ever to restart the kernel.
pub fn triple_fault() {
    // see handlers::cause_triple_fault
    unsafe { core::arch::asm!("int 255") }
}
