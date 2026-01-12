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
    kernel/src/floppy/reset.rs

    Handles sending reset commands to FDC.
    Contained within the floppy module
*/

use crate::{
    floppy::{
        FLOPPY_SPACE, FloppyCommand, FloppyError, FloppyPort,
        fifo::{self, SenseIntState},
        motor,
    },
    ports, time,
};

/// Sends a formatted configure command to the controller.
/// [`Reference - Section 5.2.7 Configure`](http://www.osdever.net/documents/82077AA_FloppyControllerDatasheet.pdf)
pub fn send_configure() -> Result<(), FloppyError> {
    /// Implied seek disabled, FIFO enabled, drive polling disabled, threshold = 8
    static COMMAND: u8 = (1 << 6) | (0 << 5) | (1 << 4) | 7;

    // Safety: Sending a well formatted configure command, see above static
    unsafe { fifo::send_command(&FloppyCommand::Configure, &[0, COMMAND, 0])? }

    Ok(())
}

/// (Re)initialises the floppy controller, which can be used to recover it after an error.
/// # Safety
/// Calling this function while disk operations are in progress may corrupt the data on the disk and CRC.
///
/// [`Reference - Section 8.2 Initialization`](http://www.osdever.net/documents/82077AA_FloppyControllerDatasheet.pdf)
pub unsafe fn init_fdc() -> Result<(), FloppyError> {
    /// Value to set the CCR to enable a 1000 Kbps datarate. Use on 2.88 Mb floppies.
    const CCR_1000_KBPS: u8 = 3;

    /// The 1000 Kbps datarate used by 2.88 Mb floppies.
    const DATARATE_1000_KBPS: u64 = 1_000_000;

    /// Value to set the CCR to enable a 500 Kbps datarate. Use on 1.44 or 1.2 Mb floppies.
    const CCR_500_KBPS: u8 = 0;

    /// The 500 Kbps datarate used by 1.44 & 1.2 Mb floppies.
    const DATARATE_500_KBPS: u64 = 500_000u64;

    motor::enable_motor()?;
    let dor = FloppyPort::DigitalOutputRegister.add_offset()?;

    // Clear the RESET bit, wait for reset to finish, then write the original val back
    // Safety: Just overwriting the DOR for a little bit to reset it, then restoring it
    unsafe {
        let prev = ports::readb(dor);
        ports::writeb(dor, 0);
        time::wait(1);
        ports::writeb(dor, prev);
    }

    // Safety: 4 sense interrupts are required after a reset
    unsafe {
        fifo::sense_interrupt(SenseIntState::FirstReset)?;
        fifo::sense_interrupt(SenseIntState::OtherReset)?;
        fifo::sense_interrupt(SenseIntState::OtherReset)?;
        fifo::sense_interrupt(SenseIntState::OtherReset)?;
    }

    // Update the wiped configuration
    send_configure()?;

    // Get the correct datarate based on the floppy's disk size
    let (datarate_val, datarate_bps) = match FLOPPY_SPACE.read()? {
        1200 | 1440 => (CCR_500_KBPS, DATARATE_500_KBPS),
        2880 => (CCR_1000_KBPS, DATARATE_1000_KBPS),
        _ => return Err(FloppyError::Other("Unsupported floppy storage capacity!")),
    };

    // Safety: The check above ensures that we're sending the right transfer speed
    unsafe { ports::writeb(FloppyPort::ConfigCtrlRegister.add_offset()?, datarate_val) }

    // Step rate time = 16 - (milliseconds * datarate / 500_000), using max possible time (8 ms)
    let srt = (16 - (8 * datarate_bps / 500000)) as u8;

    // Head load time = milliseconds * datarate / 1_000_000, using 10 ms
    let hlt = (10 * datarate_bps / 1000000) as u8;

    // Zero sets the head unload time to max possible value
    const HUT: u8 = 0;

    // Not DMA flag, disables DMA if true
    const NDMA: u8 = true as u8;

    // Send the specify command
    // Safety: Hopefully sending a formatted specify command based on the above values
    unsafe {
        fifo::send_command(
            &FloppyCommand::Specify,
            &[((srt << 4) | HUT), ((hlt << 1) | NDMA)],
        )?
    }

    motor::disable_motor();
    Ok(())
}
