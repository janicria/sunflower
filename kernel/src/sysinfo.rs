/* ---------------------------------------------------------------------------
    Sunflower kernel - sunflowerkernel.org
    Copyright (C) 2026 janicria

    This program is free software: you can redistribute it and/or modify
    it under the terms of the GNU General Public License as published by
    the Free Software Foundation, either version 3 of the License, or
    (at your option) any later version.

    This program is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
    GNU General Public License for more details.

    You should have received a copy of the GNU General Public License
    along with this program.  If not, see <https://www.gnu.org/licenses/>.
--------------------------------------------------------------------------- */

/*!
    kernel/src/sysinfo.rs

    Handles CPUID and most of the kernel's system information via the [`SystemInfo`]` type
*/

use crate::{
    floppy::{self, disk, floppyfs},
    gdt::{self, Gdt},
    interrupts::{self, Idt},
    startup::{self, ExitCode},
    time::{self, Time},
};
use core::{arch::asm, fmt::Display, sync::atomic::Ordering};
use libutil::{InitError, TableDescriptor};

/// Parses an environment variable as an int an compile time.
#[macro_export]
macro_rules! env_as_int {
    ($env: expr, $t: ty) => {
        match <$t>::from_str_radix(env!($env), 10) {
            Ok(v) => v,
            Err(_) => $crate::PANIC!(const concat!("Failed parsing env var ", $env)),
        }
    };
}

/// CPU Vendor ID returned from cpuid.
#[unsafe(no_mangle)]
static mut VENDOR: [u8; 12] = *b"Unknown     ";

/// Checks if the [cpuid](https://wiki.osdev.org/CPUID) instruction can be used.
/// # Safety
/// The [`VENDOR`] static must not be accessed anywhere during the lifetime of this function.
pub unsafe fn check_cpuid() -> ExitCode<&'static str> {
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

    ExitCode::Error("Instruction not present")
}

/// Runs cpuid and returns it's info in the `VENDOR` static.
/// # Safety
/// The cpuid instruction must be available.
#[inline(never)] // NOTE: check_cpuid expects this to NOT be inlined and page faults if it is!!
unsafe fn load_vendor() -> ExitCode<&'static str> {
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
        return ExitCode::Error("Non UTF-8 vendor ID");
    }
    ExitCode::Ok
}

/// Tries to return the value of the `VENDOR` static as a str.
fn get_cpuid() -> Option<&'static str> {
    unsafe { str::from_utf8(&*&raw const VENDOR).ok() }
}

/// Information about the system gathered from across the kernel.
pub struct SystemInfo {
    // Sunflower version
    pub sfk_version_long: &'static str,
    pub sfk_version_short: &'static str,
    pub patch_quote: &'static str,

    // Actually important info
    pub cpu_vendor: &'static str,
    pub debug: bool,

    // Floppy
    pub floppy_offset: Result<&'static u16, InitError<u16>>,
    pub floppy_space: Result<&'static u16, InitError<u16>>,
    pub floppy_drive: u8,
    pub fdc_init: bool,
    pub floppyfs_init: bool,
    pub floppy_read_bytes: u64,
    pub floppy_written_bytes: u64,

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
            // Version env vars passed via build script
            sfk_version_long: env!("SFK_VERSION_LONG"),
            sfk_version_short: env!("SFK_VERSION_SHORT"),
            patch_quote: env!("SFK_PATCH_QUOTE"),

            cpu_vendor: get_cpuid().unwrap_or("Unknown"),
            debug: cfg!(feature = "debug_info"),

            floppy_offset: floppy::BASE_OFFSET.read(),
            floppy_space: floppy::FLOPPY_SPACE.read(),
            floppy_drive: floppy::DRIVE_ONE.load() as u8,
            fdc_init: startup::FLOPPY_INIT.load(),
            floppyfs_init: floppyfs::FLOPPYFS_INIT.load(Ordering::Relaxed),
            floppy_read_bytes: disk::DISK_READ_BYTES.load(Ordering::Relaxed),
            floppy_written_bytes: disk::DISK_WRITTEN_BYTES.load(Ordering::Relaxed),

            time,
            time_secs: time / 100,
            date: time::LAUNCH_TIME.read(),

            gdt_init: gdt::GDT.read().is_ok(),
            gdt_descriptor: gdt::gdt_register(),
            idt_init: interrupts::IDT.read().is_ok(),
            idt_descriptor: interrupts::idt_register(),

            disable_enter: cfg!(feature = "disable_enter"),
            pic_init: startup::PIC_INIT.load(),
            pit_init: startup::PIT_INIT.load(),
            kbd_init: startup::KBD_INIT.load(),
        }
    }
}

impl Display for SystemInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
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

        // Write floppy
        write!(
            f,
            "\nFloppy offset: 0x{:X}
Floppy space: {} kB,
Floppy drive number: {}
Floppy init: {}
Floppyfs init: {}
Floppy bytes read: {}
Floppy bytes written: {}",
            self.floppy_offset.as_ref().unwrap_or(&&0),
            self.floppy_space.as_ref().unwrap_or(&&0),
            self.floppy_drive,
            self.fdc_init,
            self.floppyfs_init,
            self.floppy_read_bytes,
            self.floppy_written_bytes
        )
    }
}
