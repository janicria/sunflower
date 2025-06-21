/// gate type = interrupt, dpl = 0, present = 1
const INTERRUPT: u8 = 0x8E;
/// gate type = trap, dpl = 0, present = 1
const TRAP: u8 = 0x8F;

pub(super) type Handler = extern "C" fn() -> !;

/// The Interrupt Descriptor Table
pub struct Idt([InterruptDescriptor; 256]);

/// The value used for the lidt instruction to load the IDT.
#[repr(C, packed)]
struct IDTDescriptor {
    size: u16,
    offset: *const Idt
}

impl Idt {
    /// Creates a new, empty table.
    pub(super) fn new() -> Self {
        Idt([InterruptDescriptor::empty(); 256])
    }

    /// Sets the table's entry with id `entry_id`
    pub(super) fn set_handler(mut self, entry_id: usize, handler: Handler) -> Self {
        self.0[entry_id] = InterruptDescriptor::new(INTERRUPT, handler);
        self
    }

    /// Loads the table into the IDTR register.
    pub(super) fn load(&self) {
        let descriptor = IDTDescriptor {
            size: (size_of::<Self>() - 1) as u16,
            offset: self
        };

        unsafe {
            core::arch::asm!("lidt ({0})", in(reg) &descriptor, options(att_syntax));
        }
    }
}

/// An entry in the `IDT`
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct InterruptDescriptor {
    offset_low: u16,           // offset bits 0..15
    selector: u16, // segment selector in the gdt
    ist: u8,                   // ist offset
    attributes: u8,            // gate type, dpl, and present
    offset_middle: u16,        // offset bits 16..31
    offset_high: u32,          // offset bits 32..63
    reserved: u32,
}

#[allow(clippy::fn_to_numeric_cast)]
impl InterruptDescriptor {
    /// Returns a new descriptor using `attr` as it's attributes and `handler` as it's offset.
    fn new(attr: u8, handler: Handler) -> Self {
        let offset_ptr = handler as u64;
        InterruptDescriptor {
            selector: load_cs(),
            offset_low: offset_ptr as u16,
            offset_middle: (offset_ptr >> 16) as u16,
            offset_high: (offset_ptr >> 32) as u32,
            ist: 0,
            attributes: attr,
            reserved: 0,
        }
    }

    /// Returns a non-present descriptor
    fn empty() -> Self {
        InterruptDescriptor {
            offset_low: 0,
            selector: 0,
            ist: 0,
            attributes: 0,
            offset_middle: 0,
            offset_high: 0,
            reserved: 0,
        }
    }
}

/// Returns the cs register.
fn load_cs() -> u16 {
    unsafe {
        let cs;
        core::arch::asm!("mov {0:x}, cs", out(reg) cs);
        cs
    }
}