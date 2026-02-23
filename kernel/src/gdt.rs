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
    kernel/src/gdt.rs

    Handles initialising the gdt and tss
*/

use core::arch::asm;
use core::mem;

use libutil::{InitError, InitLater, LoadRegisterError, TableDescriptor};
use thiserror::Error;

use crate::startup::{self, ExitCode, GDT_INIT};
use crate::{exit_on_err, interrupts};

/// The number of entries the GDT contains.
const GDT_ENTRIES: usize = 5;

/// The loaded GDT.
pub static GDT: InitLater<Gdt> = InitLater::uninit();

/// The size of the emergency stack, in bytes.
const STACK_SIZE: u64 = 2048;

/// The emergency stack given to IST 1.
static mut STACK: [u8; STACK_SIZE as usize] = [0; STACK_SIZE as usize];

/// Offset in the GDT where the kernel's code segment will be.
#[unsafe(no_mangle)]
static CODE_SEGMENT_OFFSET: u16 = 0x8;

/// Offset in the GDT where the TSS's system segment descriptor will be.
const TSS_SEGMENT_OFFSET: u64 = 0x18;

/// The Global Descriptor Table.
#[derive(Debug)]
#[repr(transparent)]
pub struct Gdt([SegmentDescriptor; GDT_ENTRIES]);

/// A segment descriptor in the GDT.
#[derive(Debug, Default)]
#[repr(transparent)]
struct SegmentDescriptor(u64);

impl SegmentDescriptor {
      /// Creates either a new data or code segment depending
      /// on if `code_segment` is set or not.
      #[rustfmt::skip]
      fn new(code_segment: bool) -> Self {
            // Code / data segment, present & long mode bits set
            SegmentDescriptor(
                  (1 << 44) |
                  (1 << 47) |
                  (1 << 53) |
                  (code_segment as u64) << 43,
            )
      }
}

/// The loaded Task State Segment.
static TSS: InitLater<Tss> = InitLater::uninit();

/// The 64 bit Task State Segment.
#[derive(Debug, Default)]
#[repr(C, packed(4))]
pub struct Tss {
      _reserved1:     u32,
      /// Stack pointers used to when a privilege level
      /// change occurs from low to high.
      privilege_ptrs: [u64; 3],
      _reserved2:     u64,
      /// The interrupt stack table.
      ist:            [u64; 7],
      _reserved3:     u64,
      _reserved4:     u16,
      iomap:          u16,
}

/// The 64 bit System Segment Descriptor.
#[derive(Debug)]
#[repr(C, packed)]
struct SystemSegmentDescriptor {
      /// The size of the TSS - 1
      limit:           u16,
      /// The first 16 bits of the pointer
      offset_very_low: u16,
      /// The second 8 bits of the pointer
      offset_low:      u8,
      /// The access byte, just some flags
      access:          u8,
      /// Extra flags and limit bits
      flags:           u8,
      /// The middle 8 bits of the pointer
      offset_medium:   u8,
      /// The last 32 bits of the pointer
      offset_high:     u32,
      _reserved:       u32,
}

impl SystemSegmentDescriptor {
      /// Creates a new descriptor from the provided TSS.
      fn new_tss(tss: &'static Tss) -> Self {
            /// Present, available 64 bit TSS
            const ACCESS: u8 = 0b1000_1001;

            let tss = tss as *const Tss as u64;

            SystemSegmentDescriptor {
                  limit:           (size_of::<Tss>() - 1) as u16,
                  offset_very_low: tss as u16,
                  offset_low:      (tss >> 16) as u8,
                  access:          ACCESS,
                  // no extra limit bits as the TSS size fits
                  // inside the first field
                  flags:           0,
                  offset_medium:   (tss >> 24) as u8,
                  offset_high:     (tss >> 32) as u32,
                  _reserved:       0,
            }
      }
}

/// Loads a new TSS into the `TSS` static.
/// Gives the first IST stack pointer it's own stack.
pub fn setup_tss() -> ExitCode<InitError<Tss>> {
      // Calculate stack start & end addresses
      let mut tss = Tss::default();
      let stack_addr = &raw const STACK as u64;
      let stack_end_addr = stack_addr + STACK_SIZE;
      dbg_info!("emergency stack at 0x{stack_addr:x} to 0x{stack_end_addr:x}");

      // Load the TSS into it's static
      tss.ist[0] = stack_end_addr;
      tss.iomap = size_of::<Tss>() as u16;
      exit_on_err!(TSS.init(tss));
      dbg_info!("TSS at 0x{:x}", &raw const TSS as u64);

      ExitCode::Ok
}

/// Loads the TSS into the task register.
pub fn load_tss() -> ExitCode<LoadTssError> {
      if !startup::GDT_INIT.load() {
            return ExitCode::Error(LoadTssError::NoGdt);
      }

      // Safety: The above check ensures the descriptor is loaded into the GDT
      unsafe {
            asm!("ltr {0:x}", in(reg) TSS_SEGMENT_OFFSET,
            options(nostack, preserves_flags))
      }

      let stored_offset: u64;
      // Safety: Just storing a value
      unsafe {
            asm!("str {}", out(reg) stored_offset,
            options(nostack, preserves_flags))
      }

      if stored_offset != TSS_SEGMENT_OFFSET {
            ExitCode::Error(LoadTssError::BadStore(stored_offset))
      } else {
            ExitCode::Ok
      }
}

#[derive(Error, Debug)]
pub enum LoadTssError {
      #[error("The GDT isn't initialised!")]
      NoGdt,

      #[error(
            "Stored TSS offset ({0}), doesn't match const \
            ({TSS_SEGMENT_OFFSET})"
      )]
      BadStore(u64),
}

/// Loads the GDT into the GDTR register.
pub fn load_gdt() -> ExitCode<LoadRegisterError<Gdt>> {
      interrupts::cli();
      let mut gdt = Gdt([const { SegmentDescriptor(0) }; GDT_ENTRIES]);

      gdt.0[1] = SegmentDescriptor::new(true); // loaded at CODE_SEGMENT_OFFSET
      gdt.0[2] = SegmentDescriptor::new(false); // <- is this needed?

      // Don't need to log an error if the read fails, since it would
      // be printed in the 'Prepared TSS load' startup task
      if let Ok(tss) = TSS.read() {
            let desc = SystemSegmentDescriptor::new_tss(tss);

            // Safety: The gdt doesn't actually need these values to be segment
            // descriptors, two back to back can instead be a single system
            // segment descriptor, like what we're doing here
            let (low, high) = unsafe {
                  mem::transmute::<
                        SystemSegmentDescriptor,
                        (SegmentDescriptor, SegmentDescriptor),
                  >(desc)
            };

            gdt.0[3] = low; // load descriptor at TSS_SEGMENT_OFFSET
            gdt.0[4] = high;
      }

      let gdt = exit_on_err!(GDT.init(gdt));
      dbg_info!("GDT loaded at 0x{:x}", gdt as *const Gdt as u64);

      let descriptor = TableDescriptor::new(gdt);
      // Safety: The GDT was just initialised above
      unsafe {
            asm!("lgdt ({0})", in(reg) &descriptor,
            options(att_syntax, nostack))
      }

      if gdt_register() != descriptor {
            return ExitCode::Error(LoadRegisterError::Store("GDT"));
      }

      reload_cs();

      // Safety: Just loaded the GDT with a code segment
      unsafe { GDT_INIT.store(true) }

      return ExitCode::Ok;

      #[inline(never)]
      extern "C" fn reload_cs() {
            // Safety: Only ever called after GDT initialisation
            unsafe {
                  asm!(
                      "push [CODE_SEGMENT_OFFSET]",
                      "lea {addr}, [rip + 55f]",    // reg = far return addr
                      "push {addr}",
                      "retfq",                      // far return, reloading CS
                      "55:",
                      addr = lateout(reg) _,
                      options(preserves_flags),
                  )
            }
      }
}

/// Returns the current value in the GDT register.
pub fn gdt_register() -> TableDescriptor<Gdt> {
      let mut gdt = TableDescriptor::invalid();
      // Safety: Just storing a value
      unsafe {
            asm!("sgdt [{}]", in(reg) (&mut gdt),
            options(preserves_flags, nostack))
      };
      gdt
}

/// Returns the current value in the Code Segment register.
pub fn cs_register() -> u16 {
      let cs;
      // Safety: Just copying over a register
      unsafe {
            asm!("mov {0:x}, cs", out(reg) cs,
            options(preserves_flags, nostack))
      }
      cs
}

#[cfg(test)]
mod tests {
      use super::*;

      /// Tests that various structs passed to hardware
      /// are the size that it expects.
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
            // if the GDT isn't init, CS may not equal CODE_SEGMENT_OFFSET
            GDT.read().unwrap();
            assert_eq!(cs_register(), CODE_SEGMENT_OFFSET)
      }

      /// Tests that TSS System Segment Descriptors actually point to the TSS.
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
      fn ist_one_points_to_emergency_stack() {
            let tss = TSS.read().unwrap();
            let stack_end_addr = &raw const STACK as u64 + STACK_SIZE;
            let ist1 = tss.ist[0];
            assert_eq!(ist1, stack_end_addr)
      }
}
