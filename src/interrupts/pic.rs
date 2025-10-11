use super::IRQ_START;
use crate::{
    ports::{Port, writeb},
    startup,
};

/// Offset to the secondary PIC from the first.
static SECONDARY_OFFSET: u8 = 8;

/// Sends the EOI command to the corresponding PIC.
#[unsafe(no_mangle)]
pub extern "C" fn eoi(irq: u8) {
    /// Command send to a PIC to tell it that the interrupt's over.
    static COMMAND: u8 = 0b100000;

    // The PICs only support 8 IRQs each (0-15)
    if irq > 15 {
        warn!("an unknown irq ({irq}) attempted sending an EOI command!");
        return;
    }

    // Safety: Sending a valid command to the correct PIC.
    unsafe {
        if irq >= SECONDARY_OFFSET {
            writeb(Port::SecondaryPicCmd, COMMAND);
        }
        writeb(Port::MainPicCmd, COMMAND);
    }
}

/// Initialises the main and secondary PICs.
/// [`Reference`](https://wiki.osdev.org/8259_PIC)
pub fn init() {
    /// Initialisation & ICW4 bits set respectively
    static INIT_CMD: u8 = 0b10001;

    /// Tells the PIC to use 8086 mode instead of 8080.
    static MODE_8086: u8 = 0x01;

    /// The IRQ used to forward ints from the secondary to main PICs.
    static FORWARD_IRQ: u8 = 2;

    unsafe {
        // Send the init command
        writeb(Port::MainPicCmd, INIT_CMD);
        writeb(Port::SecondaryPicCmd, INIT_CMD);

        // Tell the PICs where their offsets are in the IDT
        writeb(Port::MainPicData, IRQ_START as u8);
        writeb(Port::SecondaryPicData, IRQ_START as u8 + SECONDARY_OFFSET);

        // Tell main that the secondary will be sending ints via FORWARD_IRQ
        writeb(Port::MainPicData, 1 << FORWARD_IRQ);

        // Tell the secondary that it'll be sending ints to the main via FORWARD_IRQ
        writeb(Port::SecondaryPicData, FORWARD_IRQ);

        // Tell them to use 8086 mode
        writeb(Port::MainPicData, MODE_8086);
        writeb(Port::SecondaryPicData, MODE_8086);

        // Unmask both of the PICs to allow interrupts through
        writeb(Port::MainPicData, 0);
        writeb(Port::SecondaryPicData, 0);

        startup::PIC_INIT.store(true);
    };
}
