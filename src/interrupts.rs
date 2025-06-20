use idt::Idt;
use spin::Lazy;

mod handlers;
mod idt;

static IDT: Lazy<Idt> = Lazy::new(|| handlers::add_handlers(Idt::new()));

pub fn init() {
    IDT.load();
    println!("Initialised IDT")
}
