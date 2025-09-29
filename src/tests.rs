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

    // Tests that stack overflows cause a double fault.
    // Since this 'test' causes a double fault and prevents all other tests
    // from being run, the double fault handler exits QEMU when running tests
    loop {
        unsafe { asm!("push rax") }
    }
}
