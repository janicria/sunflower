use idt::Idt;
use spin::Lazy;

mod handlers;
mod idt;
mod keyboard;
mod pic;

static IDT: Lazy<Idt> = Lazy::new(|| handlers::add_handlers(Idt::new()));

/// Loads the IDT and initialises the PIC.
pub fn init() {
    IDT.load();
    pic::init()
}

/// Causes a triple fault.
/// Can be used as the stupidest way ever to restart the kernel.
pub fn triple_fault() {
    unsafe {
        core::ptr::write(IDT.as_mut_ptr(), Idt::new()); // nuke IDT
        core::intrinsics::unreachable() // cause double fault which escalates to a triple fault
    }
}

/// Equals `true` if the bit-th last bit in val is set
#[macro_export]
macro_rules! bit_eq {
    ($val: expr, $bit: expr, $true: expr, $false: expr) => {
        if $val == $val | 0b0000_0001 << $bit - 1 {
            $true
        } else {
            $false
        }
    };
}
