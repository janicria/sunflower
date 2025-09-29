use crate::{
    gdt::{self, Gdt},
    interrupts::{self, Idt},
    startup,
    time::{self, Time},
    wrappers::{InitError, TableDescriptor},
};
use core::{arch::asm, fmt::Display};

/// The current version of the sunflower kernel.
static VERSION_LONG: &str = "SFK-00-Development-06";

/// A shortened version of the kernel's version.
static VERSION_SHORT: &str = "SFK-Dev-06";

/// Message updated each patch.
static PATCH_QUOTE: &str = "SFK > WinNT";

/// CPU Vendor ID returned from cpuid.
#[unsafe(no_mangle)]
static mut VENDOR: [u8; 12] = *b"Unknown     ";

/// Checks if the cpuid instruction can be used.
/// [`Reference`](https://wiki.osdev.org/CPUID#How_to_use_CPUID)
pub fn check_cpuid() -> Result<(), &'static str> {
    unsafe {
        asm!(
            "push rax",                        // save rax
            "pushf",                           // store eflags
            "pushf",                           // store again due to popping it again later
            "xor dword ptr [rsp], 0x00200000", // invert id bit
            "popf",                            // load flags with inverted id bit
            "pushf",                           // store eflags with inverted bit if cpuid is supported
            "pop rax",                         // rax = eflags with inverted id bit
            "xor rax, [rsp]",                  // rax = modified bits
            "popf",                            // restore eflags
            "and rax, 0x00200000",             // if rax != 0 cpuid is supported
            "cmp rax, 0",                      // check if rax == 0
            "pop rax",                         // restore rax
            "jne {}",                          // if not, we can use cpuid
            label { unsafe { return load_vendor() } }
        )
    };

    Err("Instruction not present")
}

/// Runs cpuid and returns it's info in the `VENDOR` static.
/// # Safety
/// The cpuid instruction must be available.
unsafe fn load_vendor() -> Result<(), &'static str> {
    /// Where eax, ebx, edx, ecx and rbx are saved during cpuid.
    #[unsafe(no_mangle)]
    static mut REG_BKP: [u32; 4] = [0; 4];

    macro_rules! xchg_regs {
        () => {
            "xchg eax, [REG_BKP + 0]
            xchg ebx,  [REG_BKP + 1]
            xchg edx,  [REG_BKP + 2]
            xchg ecx,  [REG_BKP + 3]"
        };
    }

    // Load cpuid into static
    unsafe {
        asm!(
            "push rbx",
            xchg_regs!(),            // save regs
            "cpuid",                 // the actual instruction
            "mov [VENDOR + 0], ebx", // first 4 letters
            "mov [VENDOR + 4], edx", // next 4 letters
            "mov [VENDOR + 8], ecx", // last 4 letters
            xchg_regs!(),            // restore regs
            "pop rbx",
            options(preserves_flags)
        )
    };

    if get_cpuid().is_none() {
        return Err("Invalid vendor ID");
    }

    Ok(())
}

/// Tries to return the value of the `VENDOR` static as a str.
fn get_cpuid() -> Option<&'static str> {
    unsafe { str::from_utf8(&*&raw const VENDOR).ok() }
}

/// Information about the system.
pub struct SystemInfo {
    // Sunflower version
    pub sfk_version_long: &'static str,
    pub sfk_version_short: &'static str,
    pub patch_quote: &'static str,

    // Actually important info
    pub cpu_vendor: &'static str,
    pub debug: bool,

    // Time
    pub time: u64,
    pub time_secs: u64,
    pub date: Result<&'static Time, InitError<Time>>,

    // Descriptors and such
    pub gdt_init: bool,
    pub gdt_descriptor: TableDescriptor<Gdt>,
    pub idt_init: bool,
    pub idt_descriptor: TableDescriptor<Idt>,

    // Misc flags
    pub pic_init: bool,
    pub pit_init: bool,
    pub kbd_init: bool,
    pub disable_enter: bool,
}

impl SystemInfo {
    /// Returns the current info about the system.
    pub fn now() -> Self {
        let time = time::get_time();

        SystemInfo {
            sfk_version_long: VERSION_LONG,
            sfk_version_short: VERSION_SHORT,
            patch_quote: PATCH_QUOTE,

            cpu_vendor: get_cpuid().unwrap_or("Unknown"),
            debug: cfg!(feature = "debug_info"),

            time,
            time_secs: time / 100,
            date: time::LAUNCH_TIME.read(),

            gdt_init: gdt::GDT.read().is_ok(),
            gdt_descriptor: gdt::gdt_register(),
            idt_init: interrupts::IDT.read().is_ok(),
            idt_descriptor: interrupts::idt_register(),

            disable_enter: cfg!(feature = "disable_enter"),
            pic_init: startup::pic_init(),
            pit_init: startup::pit_init(),
            kbd_init: startup::kbd_init(),
        }
    }
}

impl Display for SystemInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        /// Returns the value in register `reg`.
        macro_rules! reg {
    ($name: ident) => {{
        let reg: u64;
        // Safety: Just copying over a register
        unsafe { core::arch::asm!(concat!("mov {}, ", stringify!($name)), out(reg) reg, options(preserves_flags, nostack)) }
        reg
     }};
}

        // Write the first few fields
        write!(
            f,
            "Sunflower version: {}
CPU Vendor: {}
Debug build: {}
Launch time: ",
            self.sfk_version_long, self.cpu_vendor, self.debug,
        )?;

        // Write launch time
        match self.date {
            Ok(time) => writeln!(f, "{time}"),
            Err(ref e) => writeln!(f, "Failed fetching time - {e}"),
        }?;

        // Write the rest of the fields
        write!(
            f,
            "Uptime: {} ({}h {}m {}s)

Flags
Disable enter: {}
PIC initialised: {}
PIT initialised: {}
KBD initialised: {}
GDT init: {} with {}
IDT init: {} with {}\n",
            self.time,
            self.time_secs / 3600,      // hours
            (self.time_secs / 60) % 60, // mins
            self.time_secs % 60,        // secs
            self.disable_enter,
            self.pic_init,
            self.pit_init,
            self.kbd_init,
            self.gdt_init,
            self.gdt_descriptor,
            self.idt_init,
            self.idt_descriptor,
        )?;

        // Write registers
        write!(
            f,
            "\nRegisters
RAX={:x} RBX={:x} RCX={:x} RDX={:x}
RSP={:x} RSI={:x} RDI={:x} RBP={:x}
R8 ={:x} R9 ={:x} R10={:x} R11={:x} 
R12={:x} R13={:x} R14={:x} R15={:x} 
CS={:x} DS={:x} ES={:x} DS={:x}",
            reg!(rax),
            reg!(rbx),
            reg!(rcx),
            reg!(rdx),
            reg!(rsp),
            reg!(rsi),
            reg!(rdi),
            reg!(rsi),
            reg!(rbp),
            reg!(r8),
            reg!(r9),
            reg!(r10),
            reg!(r11),
            reg!(r13),
            reg!(r14),
            reg!(r15),
            reg!(cs),
            reg!(ds),
            reg!(es),
            reg!(ds)
        )
    }
}
