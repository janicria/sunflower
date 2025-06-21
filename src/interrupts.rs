use idt::Idt;
use spin::Lazy;

mod handlers;
mod idt;

static IDT: Lazy<Idt> = Lazy::new(|| handlers::add_handlers(Idt::new()));

/// Creates the IDT.
pub fn init() {
    IDT.load();
    println!("Initialised IDT")
}

/// Causes a triple fault.
/// Can be used as the stupidest way ever to restart the kernel.
pub fn triple_fault() {
    // see handlers::cause_triple_fault
    unsafe { core::arch::asm!("int 255") }
}
