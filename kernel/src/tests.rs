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
    kernel/src/tests.rs

    Handles running tests and writing to serial ports.
    Only compiled on test builds
*/

use crate::ports::{self, Port};
use core::{any, arch::asm};
use uart_16550::SerialPort;

/// Test functions marked with the `#[test_case]` attribute
pub trait Test {
    fn test(&self);
}

impl<T: Fn()> Test for T {
    fn test(&self) {
        let name = any::type_name::<Self>();
        print!("test {name} ");
        self();
        println!("- passed")
    }
}

/// Returns serial port `0x3F8` as a `SerialPort`.
fn serial_port1() -> SerialPort {
    // Safety: Using a valid serial port device
    unsafe { SerialPort::new(Port::SerialPort1 as u16) }
}

/// Writes `s` to serial port `0x3F8`.
pub fn write_serial(s: &str) {
    for byte in s.bytes() {
        serial_port1().send(byte);
    }
}

/// Exits QEMU, returning an error if `error` is set.
pub fn exit_qemu(error: bool) -> ! {
    /// The exit code marked as a successful exit by QEMU.
    static SUCCESS_EXIT_CODE: u8 = 0x42;

    // Safety: Port 0xF4 can be used when ran in tests to exit QEMU with a one byte error code
    unsafe { ports::writeb(Port::QemuExit, SUCCESS_EXIT_CODE + error as u8) }

    // If QEMU fails to exit, just loop until the tests timeout
    crate::hang()
}

/// Runs all of the tests.
pub fn run_tests(tests: &[&dyn Test]) -> ! {
    serial_port1().init();
    println!("\nRunning unit tests...");
    tests.iter().for_each(|f| f.test());

    // Tests that stack overflows cause a page fault.
    // Since this 'test' causes a page fault and prevents all other tests
    // from being run, PANIC! exits QEMU when hitting page faults in test builds
    loop {
        unsafe { asm!("push rax") }
    }
}
