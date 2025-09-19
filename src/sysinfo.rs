use crate::{startup, time};
use core::{arch::asm, fmt::Display, sync::atomic::Ordering};

/// The current version of the sunflower kernel.
static VERSION_LONG: &str = "SFK-00-Development-04";

/// A shortened version of sunflower.
static VERSION_SHORT: &str = "SFK-Dev-04";

/// Message updated each patch.
static PATCH_QUOTE: &str = "Have fun!  ";

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
    pub sfk_version_long: &'static str,
    pub sfk_version_short: &'static str,
    pub patch_quote: &'static str,
    pub cpu_vendor: &'static str,
    pub debug: bool,
    pub time: u64,
    pub time_secs: u64,
    pub disable_enter: bool,
    pub waiting: bool,
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
            debug: cfg!(debug_assertions), // compile error in main.rs should prevent this
            time,
            time_secs: time / 100,
            disable_enter: cfg!(feature = "disable_enter"),
            waiting: time::WAITING.load(Ordering::Relaxed),
        }
    }
}

impl Display for SystemInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Write the first few fields
        write!(
            f,
            "Sunflower version: {}
Debug build: {}
CPU Vendor: {}
Launch time: ",
            self.sfk_version_long, self.debug, self.cpu_vendor,
        )?;

        // Write launch time
        match time::LAUNCH_TIME.read() {
            Ok(time) => writeln!(f, "{time}"),
            Err(e) => writeln!(f, "Failed fetching time - {e}"),
        }?;

        // Write the rest of the fields
        write!(
            f,
            "Uptime: {} ({}h {}m {}s)
PIC initialised: {}
PIT initialised: {}
PS/2 Keyboard initialised: {}
Disable enter: {}
Waiting: {}",
            self.time,
            self.time_secs / 3600,      // hours
            (self.time_secs / 60) % 60, // mins
            self.time_secs % 60,        // secs
            startup::pic_init(),
            startup::pit_init(),
            startup::kbd_init(),
            self.disable_enter,
            self.waiting
        )
    }
}
