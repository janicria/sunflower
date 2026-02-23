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

    Handles CPUID and most of the kernel's
    system information via the [`SystemInfo`]` type
*/

use core::arch::asm;
use core::fmt::Display;
use core::sync::atomic::Ordering;

use libutil::{InitError, TableDescriptor};

use crate::floppy::{self, disk, floppyfs};
use crate::gdt::{self, Gdt};
use crate::interrupts::{self, Idt};
use crate::startup::{self, ExitCode};
use crate::time::{self, Time};

/// Parses an environment variable as an int an compile time.
#[macro_export]
macro_rules! env_as_int {
    ($env: expr, $t: ty) => {
        match <$t>::from_str_radix(env!($env), 10) {
            Ok(v) => v,
            Err(_) => {
                $crate::PANIC!(const concat!("Failed parsing env var ", $env))
            },
        }
    };
}

/// Checks if the cpuid  instruction can be used.
///
/// # Safety
/// Only run once at startup.
pub unsafe fn check_cpuid() -> ExitCode<&'static str> {
      #[unsafe(export_name = "reg_bkp")]
      static mut REG_BKP: [u32; 6] = [0; 6];

      macro_rules! xchg_regs {
            () => {
                  "
            xchg edx,  [reg_bkp + 0]
            xchg ecx,  [reg_bkp + 1]
            xchg rax,  [reg_bkp + 2]
            xchg rbx,  [reg_bkp + 4]"
            };
      }

      unsafe {
            asm!(
            xchg_regs!(),
            "pushf",
            "pushf",
            "xor dword ptr [rsp], 0x00200000", // invert id bit
            "popf",                // load eflags with inverted bit
            "pushf",               // bit will remain inverted if cpuid
            "pop rax",
            "xor rax, [rsp]",      // rax = modified bits
            "popf",                // restore eflags
            "and rax, 0x00200000", // cpuid is supported if rax != 0
            "cmp rax, 0",
            "je {}",               // fail if not supported
            label { return ExitCode::Error("Instruction not present") },
            options(preserves_flags)
            )
      };

      unsafe {
            asm!(
                  "cpuid",
                  "mov [cpuid_vendor + 0], ebx",
                  "mov [cpuid_vendor + 4], edx",
                  "mov [cpuid_vendor + 8], ecx",
                  xchg_regs!(),
                  options(preserves_flags)
            )
      };

      ExitCode::Ok
}

/// Returns the cpuid vendor string.
pub fn get_vendor_str() -> &'static str {
      #[unsafe(export_name = "cpuid_vendor")]
      static mut VENDOR: [u8; 12] = *b"Unknown     ";
      // Safety: VENDOR is only ever written to once at startup
      let v = unsafe { &*&raw const VENDOR };

      if let Ok(s) = str::from_utf8(v) {
            s
      } else {
            "Unknown VStr"
      }
}

/// Information about the system gathered from across the kernel.
pub struct SystemInfo {
      // Sunflower version
      pub sfk_version: &'static str,
      pub patch_quote: &'static str,

      // Actually important info
      pub cpu_vendor: &'static str,
      pub debug:      bool,

      // Floppy
      pub floppy_offset:        Result<&'static u16, InitError<u16>>,
      pub floppy_space:         Result<&'static u16, InitError<u16>>,
      pub floppy_drive:         u8,
      pub fdc_init:             bool,
      pub floppyfs_init:        bool,
      pub floppy_read_bytes:    u64,
      pub floppy_written_bytes: u64,

      // Time
      pub time:      u64,
      pub time_secs: u64,
      pub date:      Result<&'static Time, InitError<Time>>,

      // Descriptors and such
      pub gdt_init:       bool,
      pub gdt_descriptor: TableDescriptor<Gdt>,
      pub idt_init:       bool,
      pub idt_descriptor: TableDescriptor<Idt>,

      // Misc flags
      pub pic_init:      bool,
      pub pit_init:      bool,
      pub kbd_init:      bool,
      pub disable_enter: bool,
}

impl SystemInfo {
      /// Returns the current info about the system.
      pub fn now() -> Self {
            let time = time::get_time();

            SystemInfo {
                  // Version env vars passed via build script
                  sfk_version: env!("SFK_VERSION"),
                  patch_quote: env!("SFK_PATCH_QUOTE"),

                  cpu_vendor: get_vendor_str(),
                  debug: cfg!(feature = "debug_info"),

                  floppy_offset: floppy::BASE_OFFSET.read(),
                  floppy_space: floppy::FLOPPY_SPACE.read(),
                  floppy_drive: floppy::DRIVE_ONE.load() as u8,
                  fdc_init: startup::FLOPPY_INIT.load(),
                  floppyfs_init: floppyfs::FLOPPYFS_INIT
                        .load(Ordering::Relaxed),
                  floppy_read_bytes: disk::READ_BYTES.load(Ordering::Relaxed),
                  floppy_written_bytes: disk::WRITTEN_BYTES
                        .load(Ordering::Relaxed),

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
            write!(
                  f,
                  "Sunflower version: {}
CPU Vendor: {}
Debug build: {}
Launch time: ",
                  self.sfk_version, self.cpu_vendor, self.debug,
            )?;

            match self.date {
                  Ok(time) => writeln!(f, "{time}"),
                  Err(ref e) => writeln!(f, "Failed fetching time - {e}"),
            }?;

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
