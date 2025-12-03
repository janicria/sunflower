use crate::{
    interrupts,
    startup::{self, GDT_INIT},
};
use core::{arch::asm, mem};
use libutil::{InitError, InitLater, LoadRegisterError, TableDescriptor};

/// The number of entries the GDT contains.
static GDT_ENTRIES: usize = 5;

/// The loaded GDT.
pub static GDT: InitLater<Gdt> = InitLater::uninit();

/// The size of the emergency stack, in bytes.
static STACK_SIZE: u64 = 2048;

/// The emergency stack given to IST 1.
static mut STACK: [u8; STACK_SIZE as usize] = [0; STACK_SIZE as usize];

/// Offset in the GDT where the kernel's code segment will be.
#[unsafe(no_mangle)]
static CODE_SEGMENT_OFFSET: u16 = 0x8;

/// Offset in the GDT where the TSS's system segment descriptor will be.
static TSS_SEGMENT_OFFSET: u64 = 0x18;

/// The Global Descriptor Table.
/// [`Reference`](https://wiki.osdev.org/Global_Descriptor_Table)
#[derive(Debug)]
#[repr(transparent)]
pub struct Gdt([SegmentDescriptor; GDT_ENTRIES]);

/// A segment descriptor in the GDT.
/// [`Reference`](https://wiki.osdev.org/Global_Descriptor_Table#Segment_Descriptor)
#[derive(Debug, Default)]
#[repr(transparent)]
struct SegmentDescriptor(u64);

impl SegmentDescriptor {
    /// Creates either a new data or code segment based off if `code_segment` is set or not.
    fn new(code_segment: bool) -> Self {
        // Code / data segment, present & long mode bits set
        SegmentDescriptor((1 << 44) | (1 << 47) | (1 << 53) | (code_segment as u64) << 43)
    }
}

/// The loaded Task State Segment.
static TSS: InitLater<Tss> = InitLater::uninit();

/// The 64 bit Task State Segment.
/// [`Reference`](https://wiki.osdev.org/Task_State_Segment)
#[derive(Debug, Default)]
#[repr(C, packed(4))]
pub struct Tss {
    _reserved_1: u32,

    /// Stack pointers used to when a privilege level change occurs from low to high.
    privilege_ptrs: [u64; 3],
    _reserved_2: u64,

    /// The interrupt stack table.
    ist: [u64; 7],

    _reserved_3: u64,
    _reserved_4: u16,
    iomap: u16,
}

/// The 64 bit System Segment Descriptor.
/// [`Reference`](https://wiki.osdev.org/Global_Descriptor_Table#Long_Mode_System_Segment_Descriptor)
#[derive(Debug)]
#[repr(C, packed)]
struct SystemSegmentDescriptor {
    /// The size of the TSS - 1
    limit: u16,

    /// The first 16 bits of the pointer
    offset_very_low: u16,

    /// The second 8 bits of the pointer
    offset_low: u8,

    /// The access byte, just some flags
    access: u8,

    /// Extra flags and limit bits
    flags: u8,

    /// The middle 8 bits of the pointer
    offset_medium: u8,

    /// The last 32 bits of the pointer
    offset_high: u32,
    _reserved: u32,
}

impl SystemSegmentDescriptor {
    /// Creates a new descriptor from the provided TSS.
    fn new_tss(tss: &'static Tss) -> Self {
        /// Present, available 64 bit TSS
        static ACCESS: u8 = 0b1000_1001;

        let tss = tss as *const Tss as u64;

        SystemSegmentDescriptor {
            limit: (size_of::<Tss>() - 1) as u16,
            offset_very_low: tss as u16,
            offset_low: (tss >> 16) as u8,
            access: ACCESS,
            flags: 0, // no extra limit bits as the TSS size fits inside the first field
            offset_medium: (tss >> 24) as u8,
            offset_high: (tss >> 32) as u32,
            _reserved: 0,
        }
    }
}

/// Loads a new TSS into the `TSS` static.
/// Gives the first IST stack pointer it's own stack.
pub fn setup_tss() -> Result<(), InitError<Tss>> {
    // Calculate stack start & end addresses
    let mut tss = Tss::default();
    let stack_addr = &raw const STACK as u64;
    let stack_end_addr = stack_addr + STACK_SIZE;
    dbg_info!("emergency stack at 0x{stack_addr:x} to 0x{stack_end_addr:x}");

    // Load the TSS into it's static
    tss.ist[0] = stack_end_addr;
    tss.iomap = size_of::<Tss>() as u16;
    TSS.init(tss)?;
    dbg_info!("TSS at 0x{:x}", &raw const TSS as u64);

    Ok(())
}

/// Loads the TSS into the task register.
pub fn load_tss() -> Result<(), LoadRegisterError<Tss>> {
    // Bail if no TSS or no GDT
    TSS.read()?;
    if !startup::gdt_init() {
        do yeet LoadRegisterError::Other("GDT is not initialised!!!")
    }

    // Safety: The TSS descriptor is loaded into a valid GDT by this point
    unsafe { asm!("ltr {0:x}", in(reg) TSS_SEGMENT_OFFSET, options(nostack, preserves_flags)) }

    let stored_offset: u64;
    // Safety: Just storing a value into a local var
    unsafe { asm!("str {}", out(reg) stored_offset, options(nostack, preserves_flags)) }

    // Check if TSS_SEGMENT_OFFSET was actually stored
    if stored_offset != TSS_SEGMENT_OFFSET {
        do yeet LoadRegisterError::Store("TSS offset")
    }

    Ok(())
}

/// Loads the GDT into the GDTR register.
pub fn load_gdt() -> Result<(), LoadRegisterError<Gdt>> {
    interrupts::cli();
    let mut gdt = Gdt([const { SegmentDescriptor(0) }; GDT_ENTRIES]);

    // Add a code & data segment
    gdt.0[1] = SegmentDescriptor::new(true); // Loaded at CODE_SEGMENT_OFFSET
    gdt.0[2] = SegmentDescriptor::new(false); // <- is this needed?

    // Add TSS descriptor
    // Don't need to log an error if the read fails, since it would be printed in the 'Prepared TSS load' startup task
    if let Ok(tss) = TSS.read() {
        let desc = SystemSegmentDescriptor::new_tss(tss);

        // Safety: The gdt doesn't actually need these values to be segment descriptors,
        // two back to back can instead be a single system segment descriptor, like what we're doing here
        let (low, high) = unsafe {
            mem::transmute::<SystemSegmentDescriptor, (SegmentDescriptor, SegmentDescriptor)>(desc)
        };

        // Load the TSS descriptor at TSS_SEGMENT_OFFSET
        gdt.0[3] = low;
        gdt.0[4] = high;
    }

    // Load the GDT into the static
    let _gdt = GDT.init(gdt)?;
    dbg_info!("GDT loaded at 0x{:x}", _gdt as *const Gdt as u64);

    // Load the GDT to it's register
    let descriptor = TableDescriptor::new(GDT.read()?);
    // Safety: The GDT and it's descriptor MUST be valid by this point
    unsafe { asm!("lgdt ({0})", in(reg) &descriptor, options(att_syntax, nostack)) }

    if gdt_register() != descriptor {
        do yeet LoadRegisterError::Store("GDT");
    }

    // Safety: Just loaded the GDT with a code segment
    unsafe {
        reload_cs();
        GDT_INIT.store(true)
    }

    Ok(())
}

/// Returns the current value in the GDT register.
pub fn gdt_register() -> TableDescriptor<Gdt> {
    let mut gdt = TableDescriptor::invalid();
    // Safety: We're just storing a value
    unsafe { asm!("sgdt [{}]", in(reg) (&mut gdt), options(preserves_flags, nostack)) };
    gdt
}

/// Returns the current value in the Code Segment register.
pub fn cs_register() -> u16 {
    let cs;
    // Safety: We're just copying over a register
    unsafe { asm!("mov {0:x}, cs", out(reg) cs, options(preserves_flags, nostack)) }
    cs
}

/// Reloads the CS register
/// # Safety
/// There must be a valid code segment where the `CODE_SEGMENT_OFFSET` static is pointing in the GDT.
unsafe extern "C" fn reload_cs() {
    unsafe {
        asm!(
            "push [CODE_SEGMENT_OFFSET]", // push code segment offset
            "lea {addr}, [rip + 55f]",    // load far return addr into rax
            "push {addr}",                // push far return addr to the stack
            "retfq",                      // perform a far return, reloading CS
            "55:",
            addr = lateout(reg) _,
            options(preserves_flags),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that various structs passed to the CPU are the size that the CPU expects them.
    #[test_case]
    fn structs_have_the_right_size() {
        let segment_size = size_of::<SegmentDescriptor>();
        assert_eq!(size_of::<Tss>(), 104);
        assert_eq!(segment_size, 8);
        assert_eq!(size_of::<SystemSegmentDescriptor>(), segment_size * 2);
        assert_eq!(size_of::<Gdt>(), segment_size * GDT_ENTRIES)
    }

    /// Tests that the CS register equals the `CODE_SEGMENT_OFFSET` static.
    #[test_case]
    fn cs_equals_static() {
        GDT.read().unwrap(); // if the GDT isn't init, CS may not equal CODE_SEGMENT_OFFSET
        assert_eq!(cs_register(), CODE_SEGMENT_OFFSET)
    }

    /// Tests that a TSS System Segment Descriptor actually points to the TSS.
    #[test_case]
    fn tss_segment_has_correct_ptr() {
        let tss = TSS.read().unwrap();
        let ptr = tss as *const Tss as u64;
        let segment = SystemSegmentDescriptor::new_tss(tss);

        let mut segment_ptr = segment.offset_very_low as u64;
        segment_ptr |= (segment.offset_low as u64) << 16;
        segment_ptr |= (segment.offset_medium as u64) << 24;
        segment_ptr |= (segment.offset_high as u64) << 32;
        assert_eq!(ptr, segment_ptr)
    }

    /// Tests that IST 1 points to the emergency stack.
    #[test_case]
    fn ist_one_points_to_df_stack() {
        let tss = TSS.read().unwrap();
        let stack_end_addr = &raw const STACK as u64 + STACK_SIZE;
        let ist1 = tss.ist[0];
        assert_eq!(ist1, stack_end_addr)
    }
}
