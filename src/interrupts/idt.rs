use x86::Ring;
use x86::segmentation::{self, SegmentSelector};

/// gate type = interrupt, dpl = 0, present = 1
const INTERRUPT: u8 = 0x8E;
/// gate type = trap, dpl = 0, present = 1
const TRAP: u8 = 0x8F;

pub(super) type HandlerFunc = extern "C" fn() -> !;

/// The Interrupt Descriptor Table
pub struct Idt([InterruptDescriptor; 16]);

impl Idt {
    /// Creates a new, empty table.
    pub(super) fn new() -> Self {
        Idt([InterruptDescriptor::empty(); 16])
    }

    /// Sets the table's entry with id `entry_id`
    pub(super) fn set_handler(mut self, entry_id: usize, handler: HandlerFunc) -> Self {
        self.0[entry_id] = InterruptDescriptor::new(INTERRUPT, handler);
        self
    }

    /// Loads the table into the IDTR register.
    pub(super) fn load(&self) {
        use x86::dtables::{DescriptorTablePointer, lidt};

        let ptr = DescriptorTablePointer {
            base: self,
            limit: (size_of::<Self>() - 1) as u16,
        };

        unsafe {
            lidt(&ptr);
        }
    }
}

/// An entry in the `IDT`
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct InterruptDescriptor {
    offset_low: u16,           // offset bits 0..15
    selector: SegmentSelector, // segment selector in the gdt
    ist: u8,                   // ist offset
    attributes: u8,            // gate type, dpl, and present
    offset_middle: u16,        // offset bits 16..31
    offset_high: u32,          // offset bits 32..63
    reserved: u32,
}

impl InterruptDescriptor {
    /// Returns a new descriptor using `attr` as it's attributes and `handler` as it's offset.
    fn new(attr: u8, handler: HandlerFunc) -> Self {
        let offset_ptr = handler as u64;
        InterruptDescriptor {
            selector: segmentation::cs(),
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
            selector: SegmentSelector::new(0, Ring::Ring0),
            ist: 0,
            attributes: 0,
            offset_middle: 0,
            offset_high: 0,
            reserved: 0,
        }
    }
}
