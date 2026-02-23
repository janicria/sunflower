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
    kernel/src/time.rs

    Handles the i8253/i8254 PIT and the RTC
*/

use core::arch::naked_asm;
use core::fmt::Display;
use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use core::{hint, ptr};

use libutil::InitLater;
use thiserror::Error;

use crate::interrupts;
use crate::ports::{self, Port};
use crate::startup::{self, ExitCode};
use crate::vga::print::{Color, Corner, VGAChar};

/// The base frequency of the PIT.
pub const PIT_BASE_FREQ: u64 = 1193180;

/// The time the kernel was launched.
pub static LAUNCH_TIME: InitLater<Time> = InitLater::uninit();

/// Whether the time has been loaded into `LAUNCH_TIME` or not.
static RTC_SYNC_DONE: AtomicBool = AtomicBool::new(false);

/// CMOS register B.
const CMOS_REG_B: u8 = 0x8B;

/// The waiting character is only able to be toggled when this static is.
pub static WAITING_CHAR: AtomicBool = AtomicBool::new(true);

/// Sets the timer interval in channel 0 to 10 ms.
#[rustfmt::skip]
pub fn set_timer_interval() -> ExitCode<&'static str> {
      /// How many kernel ticks we want per second.
      const KERNEL_TICKS_HZ: u64 = 100;

      const TICK_INTERVAL: u16 =
         ((PIT_BASE_FREQ + KERNEL_TICKS_HZ/2) / KERNEL_TICKS_HZ) as u16;

      /// Binary mode, square wave, low & high byte, channel 0
      const COMMAND: u8 = 0b0_111_11_00;

      if !startup::PIC_INIT.load() {
            return ExitCode::Error("The PIC isn't init!");
      }

      interrupts::sti();

      // Safety: Sending valid command
      unsafe {
            ports::writeb(Port::PITCmd, COMMAND);
            ports::writeb(Port::PITChannel0, TICK_INTERVAL as u8); // low byte
            ports::writeb(Port::PITChannel0, (TICK_INTERVAL >> 8) as u8); // high byte
      }

      // Safety: Was just initialised above
      unsafe { startup::PIT_INIT.store(true) }

      ExitCode::Infallible
}

/// Returns how many ticks the kernel has been running for.
/// Increases every 10 ms or 100 Hz.
#[unsafe(naked)]
pub extern "sysv64" fn get_time() -> u64 {
      #[unsafe(no_mangle)]
      static mut TIME: u64 = 0;

      // Safety: Just checking the time
      naked_asm!("mov rax, [TIME]", "ret")
}

/// Toggles the waiting character on or off.
pub fn set_waiting_char(show: bool) {
      if !WAITING_CHAR.load(Ordering::Relaxed) {
            return;
      }

      const CHAR: u16 = VGAChar::new(1, Color::Black, Color::LightGrey).0;
      static PREV: AtomicU16 = AtomicU16::new(0);

      let ptr = Corner::TopRight as usize as *mut u16;
      let write_char = |char: u16| {
            // Safety: TopRight is valid, aligned & won't do anything weird when
            // written to
            unsafe { ptr::write_volatile(ptr, char) }
      };

      if show {
            // Safety: Just reading from the text buffer
            let prev = unsafe { ptr::read_volatile(ptr) };
            PREV.store(prev, Ordering::Relaxed);
            write_char(CHAR);
      } else {
            let prev = PREV.load(Ordering::Relaxed);
            write_char(prev);
      }
}

/// Waits for `ticks` ticks (`ticks / 100` seconds).
///
/// Never returns if external interrupts are disabled.
pub fn wait(ticks: u64) {
      if !startup::PIT_INIT.load() {
            warn!("pit: attempted waiting ({ticks}) without a PIT!");
            return;
      }

      set_waiting_char(true);

      // wait...
      let target_time = get_time() + ticks + 1;
      while get_time() < target_time {
            interrupts::hlt();
      }

      set_waiting_char(false);
}

/// The century the kernel was complied.
/// Only updated when the kernel is built so isn't too precise.
const CENTURY: u16 = crate::env_as_int!("SFK_TIME_CENTURY", u16);

/// Second-precise time value.
#[derive(Debug, Default, Clone, Copy)]
pub struct Time {
      /// The current year, 0-65535
      year:  u16,
      /// The current month, 1-12
      month: u8,
      /// The current day of the month, 1-31
      day:   u8,
      /// The number of hours that have passed in the day, 0-23
      hour:  u8,
      /// The number of minutes that have passed in the hour, 0-59
      min:   u8,
      /// The number of seconds that have passed in the minute, 0-59
      sec:   u8,
}

impl Time {
      /// Returns the current time in the RTC.
      fn now() -> Self {
            // Safety: Reading from valid registers.
            unsafe {
                  Time {
                        year:  read_cmos_reg(0x9) as u16,
                        month: read_cmos_reg(0x8),
                        day:   read_cmos_reg(0x7),
                        hour:  read_cmos_reg(0x4),
                        min:   read_cmos_reg(0x2),
                        sec:   read_cmos_reg(0x0),
                  }
            }
      }
}

impl Display for Time {
      fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(
                  f,
                  "{}:{}:{} {}/{}/{}",
                  self.hour,
                  self.min,
                  self.sec,
                  self.day,
                  self.month,
                  self.year
            )
      }
}

/// Returns the current value of CMOS register `reg`.
/// # Safety
/// Reads and writes to I/O ports.
pub unsafe fn read_cmos_reg(reg: u8) -> u8 {
      unsafe {
            ports::writeb(Port::CMOSIndex, reg);
            ports::readb(Port::CMOSData)
      }
}

/// Sets up RTC interrupts in IRQ 8.
pub fn setup_rtc_int() -> ExitCode<&'static str> {
      if !startup::PIC_INIT.load() {
            return ExitCode::Error("The PIC isn't init!");
      }

      interrupts::cli();

      // Set bit 6 in register B to enable interrupts.
      // Safety: Sending a valid command with external interrupts disabled
      unsafe {
            let prev = read_cmos_reg(CMOS_REG_B);
            ports::writeb(Port::CMOSIndex, CMOS_REG_B);
            ports::writeb(Port::CMOSData, prev | 0b1000000);
      }

      // Safety: Just enabled it above!
      unsafe { startup::RTC_IRQ_INIT.store(true) }

      interrupts::sti();
      ExitCode::Infallible
}

/// Waits for the RTC sync to finish then checks if `LAUNCH_TIME` has been
/// successfully loaded.
pub fn wait_for_rtc_sync() -> ExitCode<RtcSyncWaitError> {
      if !startup::RTC_IRQ_INIT.load() {
            return ExitCode::Error(RtcSyncWaitError::NoIrq);
      }

      // Wait until the time has been loaded into LAUNCH_TIME
      while !RTC_SYNC_DONE.load(Ordering::Relaxed) {
            hint::spin_loop(); // better performance via pause instruction
      }

      if let Err(e) = LAUNCH_TIME.read() {
            return ExitCode::Error(RtcSyncWaitError::NoStatic(e.state));
      }

      ExitCode::Ok
}

#[derive(Error, Debug)]
pub enum RtcSyncWaitError {
      #[error("The RTC IRQ isn't enabled!")]
      NoIrq,

      #[error(
            "RTC handler failed setting launch time, static's init state is {0}"
      )]
      NoStatic(u8),
}

/// Ran by RTC handler when the update ended interrupt occurs,
/// stores the current time into [`LAUNCH_TIME`].
///
/// See https://wiki.osdev.org/CMOS#The_Real-Time_Clock
#[unsafe(no_mangle)]
extern "sysv64" fn sync_time_to_rtc() {
      /// The 24 hour time / 12 hour time flag in the hours value.
      const FLAG_24_HR: u8 = 0b10000000;

      let mut time = Time::now();
      let reg_b = unsafe { read_cmos_reg(CMOS_REG_B) };
      let mut hour = time.hour;

      // If BCD mode (bit 2 clear), convert values to binary using the formula
      // Binary = ((BCD / 16) * 10) + (BCD & 0xF)
      if reg_b != reg_b | 0b100 {
            time.sec = bcd_to_bin(time.sec);
            time.min = bcd_to_bin(time.min);
            time.day = bcd_to_bin(time.day);
            time.month = bcd_to_bin(time.month);
            time.year = bcd_to_bin(time.year as u8) as u16;

            // Preserve 24 hour flag
            hour = ((hour & 0x0F) + (((hour & 0x70) / 16) * 10)) |
                  (hour & FLAG_24_HR);
      }

      // If 12 hour time (bit 1 clear and flag set)
      if (reg_b != reg_b | 0b10) && (hour == hour & FLAG_24_HR) {
            // Clear 24 / 12 hour flag and convert to 24 hour time
            time.hour = ((hour & 0b1111111) + 12) % 24;
      }

      time.year += CENTURY * 100;

      // Ignore possible error as wait_for_rtc_sync checks this later
      _ = LAUNCH_TIME.init(time);
      RTC_SYNC_DONE.store(true, Ordering::Relaxed);

      fn bcd_to_bin(bcd: u8) -> u8 {
            ((bcd / 16) * 10) + (bcd & 0xF)
      }
}

#[cfg(test)]
mod tests {
      use super::*;
      use crate::speaker;

      /// Tests that `wait` waits for the correct amount of time.
      #[test_case]
      fn wait_waits_for_correct_time() {
            // Ensures that time doesn't increase in between getting time &
            // starting waiting
            wait(1);

            let time = get_time();
            wait(15);
            assert!(get_time() - 15 - time < 3) // less than 3 tick difference
      }

      /// Tests that `wait` & `play_special` immediately return if the PIT
      /// failed initialisation.
      #[test_case]
      fn wait_services_require_pit() {
            let init = startup::PIT_INIT.load();
            unsafe { startup::PIT_INIT.store(false) }

            // Test fails due to timeout
            wait(u64::MAX);
            speaker::play_special(0, u64::MAX, false);

            unsafe { startup::PIT_INIT.store(init) }
      }

      /// Tests that the RTC contains sane values through `LAUNCH_TIME`.
      #[test_case]
      fn rtc_contains_sane_values() {
            let time = LAUNCH_TIME.read().unwrap();
            assert!((time.year - CENTURY * 100) < 100);
            assert!(time.month != 0 && time.month <= 12);
            assert!(time.day != 0 && time.day <= 31);
            assert!(time.hour < 24);
            assert!(time.min < 60);
            assert!(time.sec < 60);
      }
}
