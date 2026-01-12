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
    kernel/src/floppy/motor.rs

    Allows enabling and disabling the currently initialised floppy's motor
    Contained within the floppy module
*/

use super::{DRIVE_ONE, FloppyPort};
use crate::{ports, time};
use core::sync::atomic::{AtomicU8, AtomicU16, Ordering};
use libutil::InitError;

/// How long is left before the floppy's motor is disabled.
static MOTOR_TIME_LEFT: AtomicU16 = AtomicU16::new(0);

/// The current state of the floppy's motor. See below consts for valid states.
static MOTOR_STATE: AtomicU8 = AtomicU8::new(MOTOR_OFF);

/// The floppy's motor is on.
const MOTOR_ON: u8 = 0;

/// The floppy's motor is waiting to be turned off.
const MOTOR_DISABLING: u8 = 1;

/// The floppy's motor is off.
const MOTOR_OFF: u8 = 2;

/// Enables the floppy's motor if it was disabled.
pub fn enable_motor() -> Result<(), InitError<u16>> {
    /// Drive 0's motor on, IRQs & DMA off, drive 0.
    /// [`Reference`](https://wiki.osdev.org/Floppy_Disk_Controller#DOR_bitflag_definitions)
    static DRIVE0_COMMAND: u8 = 0b01_0_1_00;

    /// Drive 1's motor on, IRQs & DMA off, drive 1
    static DRIVE1_COMMAND: u8 = 0b10_0_1_01;

    match MOTOR_STATE.load(Ordering::Relaxed) {
        // The motor isn't on, enable it
        MOTOR_OFF => {
            let dor_port = FloppyPort::DigitalOutputRegister.add_offset()?;

            if DRIVE_ONE.load() {
                // Safety: Check above ensure that drive 1 is being used
                unsafe { ports::writeb(dor_port, DRIVE1_COMMAND) };
            } else {
                // Safety: The check above ensure that drive 0 is being used
                unsafe { ports::writeb(dor_port, DRIVE0_COMMAND) }
            }

            MOTOR_STATE.store(MOTOR_ON, Ordering::Relaxed);
            time::wait(50); // motor can take up to 500 ms to speed up
            dbg_info!("floppy motor on!")
        }

        // The motor's already on, but waiting to be disabled
        MOTOR_DISABLING => MOTOR_STATE.store(MOTOR_ON, Ordering::Relaxed),

        // The motor was already enabled
        _ => (),
    }

    Ok(())
}

/// Enters the disabling state for the floppy's motor.
pub fn disable_motor() {
    /// Time until the motor will be disabled, in kernel ticks (10 Hz)
    // Note: Due to the fetch_sub being used in decrease_motor_time, it's actually 51 ticks
    static TIMEOUT: u16 = 50;

    MOTOR_TIME_LEFT.store(TIMEOUT, Ordering::Relaxed);
    MOTOR_STATE.store(MOTOR_DISABLING, Ordering::Relaxed);
}

/// Forcefully disables the floppy's motor.
pub fn force_disable() {
    MOTOR_STATE.store(MOTOR_DISABLING, Ordering::Relaxed);
    MOTOR_TIME_LEFT.store(0, Ordering::Relaxed);
    decrease_motor_time();
}

/// Decreases the time until the motor will be disabled.
/// Called by the timer handler every 10 ms.
#[unsafe(export_name = "dec_floppy_motor_time")]
pub extern "C" fn decrease_motor_time() {
    /// Drive 0's motor off, IRQs & DMA off, drive 0.
    /// [`Reference`](https://wiki.osdev.org/Floppy_Disk_Controller#DOR_bitflag_definitions)
    static DRIVE0_COMMAND: u8 = 0b00_0_1_00;

    /// Drive 1's motor off, IRQs & DMA off, drive 1
    static DRIVE1_COMMAND: u8 = 0b00_0_1_01;

    // If the motor's time has run out, disable it
    if MOTOR_STATE.load(Ordering::Relaxed) == MOTOR_DISABLING
        && MOTOR_TIME_LEFT.fetch_sub(1, Ordering::Relaxed) == 0
        && let Ok(dor) = FloppyPort::DigitalOutputRegister.add_offset()
    {
        dbg_info!("floppy motor off!");
        if DRIVE_ONE.load() {
            // Safety: Check above ensure that drive 1 is being used
            unsafe { ports::writeb(dor, DRIVE1_COMMAND) }
        } else {
            // Safety: The check above ensure that drive 0 is being used
            unsafe { ports::writeb(dor, DRIVE0_COMMAND) }
        }

        MOTOR_STATE.store(MOTOR_OFF, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that disable motor keeps the motor running for a brief period.
    #[test_case]
    fn disable_motor_keeps_motor_running() {
        _ = enable_motor();
        disable_motor();

        time::wait(1);
        let time: u64 = time::get_time();

        for _ in 0..16 {
            // Shouldn't wait 500-520 ms each since the motor isn't actually off
            _ = enable_motor();
        }

        time::wait(1);
        assert!(time::get_time() - time < 5); // less than 5 tick difference
        disable_motor(); // actually disable the motor
    }
}
