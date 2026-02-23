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

use crate::floppy::fifo::{self, SenseIntState};
use crate::floppy::{
      FLOPPY_SPACE, FloppyCommand, FloppyError, FloppyPort, motor,
};
use crate::{ports, time};

/// Sends the configure command to the controller.
///
/// See section 5.2.7 Configure of the datasheet.
pub fn send_configure() -> Result<(), FloppyError> {
      /// No implied seeks, FIFO enabled, drive polling disabled,
      /// 8 bytes between interrupts, keep default write precompensation.
      const PARAMS: &[u8; 3] = &[0, ((1 << 6) | (0 << 5) | (1 << 4) | 7), 0];

      // Safety: Above const is safe to send as configure parameters
      unsafe { fifo::send_command(&FloppyCommand::Configure, PARAMS)? }

      Ok(())
}

/// (Re)initialises the floppy controller, which can be used to recover it after
/// an error.
///
/// # Safety
/// Calling this function while disk operations are in progress may corrupt the
/// data on the disk and CRC.
pub unsafe fn init_fdc() -> Result<(), FloppyError> {
      /// Value to set the CCR to enable a 1000 Kbps datarate.
      /// Use on 2880 KB floppies.
      const CCR_1000_KBPS: u8 = 3;

      /// Value to set the CCR to enable a 500 Kbps datarate.
      /// Use on 1440 or 1200 KB floppies.
      const CCR_500_KBPS: u8 = 0;

      const _1000_KB: u64 = 1_000_000;
      const _500_KBPS: u64 = 500_000;

      motor::enable_motor()?;
      let dor = FloppyPort::DigitalOutputRegister.add_offset()?;

      // Safety: The DOR's state is restored after clearing
      // the reset bit and waiting for the it to finish.
      unsafe {
            let prev = ports::readb(dor);
            ports::writeb(dor, 0);
            time::wait(1);
            ports::writeb(dor, prev);
      }

      // Safety: 4 sense interrupts are required after a reset
      unsafe {
            fifo::sense_int(SenseIntState::FirstReset)?;
            fifo::sense_int(SenseIntState::OtherReset)?;
            fifo::sense_int(SenseIntState::OtherReset)?;
            fifo::sense_int(SenseIntState::OtherReset)?;
      }

      send_configure()?;

      let (ccr, datarate) = match FLOPPY_SPACE.read()? {
            1200 | 1440 => (CCR_500_KBPS, _500_KBPS),
            2880 => (CCR_1000_KBPS, _1000_KB),
            _kb => {
                  return Err(FloppyError::Other(
                        "Unsupported storage capacity: {_kb} KB",
                  ));
            }
      };

      // Safety: The check above ensures that we're sending the right speed
      unsafe {
            ports::writeb(FloppyPort::ConfigCtrlRegister.add_offset()?, ccr)
      }

      // Step rate time = (16 - (milliseconds * datarate / 500000)), max 8 ms.
      let srt = (16 - (8 * datarate / 500000)) as u8;

      // Head load time = (milliseconds * datarate / 1000000).
      let hlt = (10 * datarate / 1000000) as u8;

      /// Zero = max head unload time.
      const HUT: u8 = 0;

      /// Not DMA flag.
      const NDMA: u8 = true as u8;

      let params = &[((srt << 4) | HUT), ((hlt << 1) | NDMA)];

      // Safety: The check above ensures that we're sending the right params
      unsafe { fifo::send_command(&FloppyCommand::Specify, params)? }

      motor::disable_motor();
      Ok(())
}
