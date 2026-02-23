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
    kernel/src/interrupts/pic.rs

    Handle PIC initialisation and EOI.
    Contained within the interrupts module
*/

use core::convert::Infallible;

use super::IRQ_START;
use crate::ports::{Port, writeb};
use crate::startup::{self, ExitCode};

/// Offset to the secondary PIC from the first.
const SECONDARY_OFFSET: u8 = 8;

/// Sends the EOI command to the corresponding PIC.
#[unsafe(no_mangle)]
pub extern "sysv64" fn eoi(irq: u8) {
      /// End of interrupt command, tells PIC the interrupt's over.
      const EOI_COMMAND: u8 = 0b100000;

      // The PICs only support 8 IRQs each (0-15)
      if irq > 15 {
            warn!("pic: unknown eoi irq: {irq}");
            return;
      }

      // Safety: If the IRQ came from the main (master) PIC, the EOI must be
      // sent to the main only. However if the IRQ came from the secondary
      // (slave) PIC, the command must be sent to both
      unsafe {
            if irq >= SECONDARY_OFFSET {
                  writeb(Port::SecondaryPicCmd, EOI_COMMAND);
            }
            writeb(Port::MainPicCmd, EOI_COMMAND);
      }
}

/// Initialises the main and secondary PICs.
///
/// # Safety
/// Only run this ONCE at startup time.
pub unsafe fn init() -> ExitCode<Infallible> {
      /// Initialisation command.
      const INIT_CMD: u8 = 0b10001;

      /// Tells the PIC to use 8086 mode instead of 8080.
      const MODE_8086: u8 = 0x01;

      /// The IRQ used to forward ints from the secondary to main PICs.
      const FORWARD_IRQ: u8 = 2;

      // Safety: Caller must ensure this is only called once
      unsafe {
            writeb(Port::MainPicCmd, INIT_CMD);
            writeb(Port::SecondaryPicCmd, INIT_CMD);

            writeb(Port::MainPicData, IRQ_START as u8); // send offsets
            writeb(Port::SecondaryPicData, IRQ_START as u8 + SECONDARY_OFFSET);

            // tell both where forwarded ints will go / come from
            writeb(Port::MainPicData, 1 << FORWARD_IRQ);
            writeb(Port::SecondaryPicData, FORWARD_IRQ);

            writeb(Port::MainPicData, MODE_8086); // use 8086 mode
            writeb(Port::SecondaryPicData, MODE_8086);

            writeb(Port::MainPicData, 0); // unmask to allow ints
            writeb(Port::SecondaryPicData, 0);
      };

      // Safety: Just initialised it above
      unsafe { startup::PIC_INIT.store(true) }

      ExitCode::Infallible
}
