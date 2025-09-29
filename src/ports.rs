use core::arch::asm;

/// An I/O port required to written to or read from.
#[repr(u16)]
#[allow(unused)]
pub enum Port {
    // PIC ports
    MainPicCmd = 0x20,
    MainPicData = 0x0021,
    SecondaryPicCmd = 0xA0,
    SecondaryPicData = 0xA1,

    // PS/2 & PC speaker
    PS2Data = 0x60,
    Speaker = 0x61,

    // VGA ports
    VGAIndexRegister0x3D4 = 0x3D4,
    VgaCursorPos = 0x3D5,

    // PIT ports
    PITChannel0 = 0x40,
    PITChannel2 = 0x42,
    PITCmd = 0x43,

    // CMOS ports
    CMOSSelector = 0x70,
    CMOSRegister = 0x71,

    // Ports used by test
    QemuExit = 0xF4,
    SerialPort1 = 0x3F8,

    // Apparently rust doesn't like invalid enum values
    Unused = 0x80,
}

/// Writes `val` to port `port`.
/// # Safety
/// Writes to I/O ports.
pub unsafe fn writeb(port: Port, val: u8) {
    // Safety: The caller must ensure that writing to this port is safe
    unsafe { asm!("out dx, al", in("dx") port as u16, in("al") val, ) }
}

/// Returns the value in port `port`.
/// # Safety
/// Reads from I/O ports.
pub unsafe fn readb(port: Port) -> u8 {
    let val;
    // Safety: The caller must ensure that reading from this port is safe
    unsafe { asm!("in al, dx", out("al") val, in("dx") port as u16) }
    val
}
