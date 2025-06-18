use idt::Idt;
use spin::Lazy;

mod idt;

static IDT: Lazy<Idt> = Lazy::new(|| {
    Idt::new()
        .set_handler(0, divide_by_zero_handler)
        .set_handler(14, page_fault_handler)
});

pub fn init() {
    IDT.load();
    println!("Initialised IDT")
}

#[unsafe(no_mangle)]
extern "C" fn divide_by_zero_handler() -> ! {
    println!("EXCEPTION OCCURED: Attempted to divide by 0");
    loop {}
}

extern "C" fn page_fault_handler() -> ! {
    println!("EXCEPTION OCCURED: Page fault");
    loop {}
}
