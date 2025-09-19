use crate::{
    interrupts,
    wrappers::{InitLater, LoadDescriptorError},
};
use core::arch::asm;

/// The number of entries the GDT contains.
static GDT_ENTRIES: usize = 3;

static GDT: InitLater<Gdt> = InitLater::uninit();

/// The Global Descriptor Table.
/// [`Reference`](https://wiki.osdev.org/Global_Descriptor_Table)
pub struct Gdt([SegmentDescriptor; GDT_ENTRIES]);

/// A segment descriptor in the GDT.
/// [`Reference`](https://wiki.osdev.org/Global_Descriptor_Table#Segment_Descriptor)
#[derive(Default)]
#[repr(transparent)]
struct SegmentDescriptor(u64);

impl SegmentDescriptor {
    /// Creates either a new data or code segment based off if `code_segment` is set or not.
    fn new(code_segment: bool) -> Self {
        // Code / data segment, present & long mode bits set
        SegmentDescriptor((1 << 44) | (1 << 47) | (1 << 53) | (code_segment as u64) << 43)
    }
}

/// The value used by the lgdt instruction load the GDT.
#[derive(PartialEq, Default)]
#[repr(C, packed)]
pub struct GDTDescriptor {
    size: u16,
    offset: *const Gdt,
}

/// Loads the GDT into the GDTR register.
pub fn load_gdt() -> Result<(), LoadDescriptorError<Gdt>> {
    interrupts::cli();
    let mut gdt = Gdt([const { SegmentDescriptor(0) }; GDT_ENTRIES]);

    // Init GDT with a code & data segment
    gdt.0[1] = SegmentDescriptor::new(true);
    gdt.0[2] = SegmentDescriptor::new(false);
    GDT.init(gdt)?;

    // Descriptor to be loaded into GDTR
    let descriptor = GDTDescriptor {
        size: (size_of::<Gdt>() - 1) as u16,
        offset: GDT.read()?,
    };

    // Safety: The GDT and it's descriptor MUST be valid by this point
    unsafe {
        asm!("lgdt ({0})", in(reg) &descriptor, options(att_syntax, nostack));
    }

    // Check if the loaded GDT equals the stored one
    let mut stored_gdt = GDTDescriptor::default();
    // Safety: We're just storing a value
    unsafe { asm!("sgdt [{}]", in(reg) (&mut stored_gdt), options(nostack)) };

    if stored_gdt != descriptor {
        return Err(LoadDescriptorError::Store("GDT"));
    }

    Ok(())
}
