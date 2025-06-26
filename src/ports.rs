#[repr(u16)]
pub enum Port {
    MainPicCmd = 0x20,
    MainPicData = 0x0021,
    SecondaryPicCmd = 0x00A0,
    SecondaryPicData = 0x00A1,
    PS2Data = 0x60,
    PS2Status = 0x64,
}

/// Writes `val` to port `port`.
pub unsafe fn writeb(port: Port, val: u8) {
    unsafe { core::arch::asm!("out dx, al", in("dx") port as u16, in("al") val, ) }
}

/// Returns the value in port `port`.
pub fn readb(port: Port) -> u8 {
    let val;
    unsafe { core::arch::asm!("in al, dx", out("al") val, in("dx") port as u16) }
    val
}
