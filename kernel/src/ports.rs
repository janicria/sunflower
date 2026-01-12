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
    kernel/src/ports.rs

    Allows writing to and reading from specific I/O ports
*/

use core::arch::asm;

/// An I/O port which can be written to or read from.
#[derive(Debug, Clone, Copy)]
#[repr(u16)]
#[allow(dead_code)]
pub enum Port {
    // --- PIC ports ---
    /// The main / master PIC command port, write only
    MainPicCmd = 0x20,

    /// The main / master PIC data port, read & write
    MainPicData = 0x21,

    /// The secondary / slave PIC command port, write only
    SecondaryPicCmd = 0xA0,

    /// The secondary / slave PIC data port, read & write
    SecondaryPicData = 0xA1,

    // --- VGA ports ---
    /// Used to ito select which VGA register `VGARegisterC` is connected to, write only
    /// [`Reference`](https://wiki.osdev.org/VGA_Hardware#Port_0x3C4,_0x3CE,_0x3D4)
    VGASelectorC = 0x3D4,

    /// VGA register selected by `VGASelectorC`, read & write
    VGARegisterC = 0x3D5,

    // --- PIT ports ---
    /// Port connected to channel 0 of the PIT, read & write
    PITChannel0 = 0x40,

    /// Port connected to channel 2 of the PIT, read & write
    PITChannel2 = 0x42,

    /// Port used to send commands to the PIT, write only
    PITCmd = 0x43,

    // --- CMOS ports ---
    /// Port used to select which CMOS register `CMOSRegister` is connected to, write only
    /// [`Reference`](https://wiki.osdev.org/CMOS#CMOS_Registers)
    CMOSSelector = 0x70,

    /// CMOS register selected by `CMOSSelector`, read & write
    CMOSRegister = 0x71,

    // --- QEMU ports ---
    /// Port which can be written to when using QEMU to cause it to exit (actually Disk Controller status register)
    QemuExit = 0xF4,

    /// Serial Port 1, used to send data to QEMU
    SerialPort1 = 0x3F8,

    // --- Misc ports ---
    /// PS/2 data port, read & write
    PS2Data = 0x60,

    /// PC speaker port, read & write
    PCSpeaker = 0x61,

    /// Unused port (POST codes apparently) used for dummy waits, read & write
    Unused = 0x80,
}

impl From<Port> for u16 {
    fn from(val: Port) -> Self {
        val as u16
    }
}

/// Writes `val` to port `port` with a dummy write for a delay
/// as hardware often needs some time to actually respond to I/O ports.
/// # Safety
/// Writes to I/O ports.
pub unsafe fn writeb<P: Into<u16>>(port: P, val: u8) {
    // Safety: The caller must ensure that writing to the port is safe
    unsafe {
        writeb_nodummy(Port::Unused as u16, 0);
        writeb_nodummy(port.into(), val);
    }
}

/// Writes `val` to port `Port` without a dummy write for a delay.
unsafe fn writeb_nodummy(port: u16, val: u8) {
    // Safety: The caller must ensure that writing to this port is safe
    unsafe { asm!("out dx, al", in("dx") port, in("al") val, ) }
}

/// Returns the value in port `port`.
/// Takes an `Port` enum instead of a u16.
/// # Safety
/// Reads from I/O ports.
pub unsafe fn readb<P: Into<u16>>(port: P) -> u8 {
    let val;

    // Safety: The caller must ensure that reading from this port is safe
    unsafe {
        writeb_nodummy(Port::Unused as u16, 0);
        asm!("in al, dx", out("al") val, in("dx") port.into())
    }

    val
}
