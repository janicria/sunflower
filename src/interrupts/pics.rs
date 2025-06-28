use crate::{ports::{writeb, Port}, vga};

const MAIN_OFFSET: u8 = 32;
const SECONDARY_OFFSET: u8 = 40;
const INIT_CMD: u8 = 0x10 | 0x01;
const EOI_CMD: u8 = 0x20;
const MODE_8086: u8 = 0x01;

#[unsafe(no_mangle)]
pub extern "C" fn eoi(irq: u8) {
    unsafe {
        if irq >= 8 {
            writeb(Port::SecondaryPicCmd, EOI_CMD);
        }
        writeb(Port::MainPicCmd, EOI_CMD);
    }
}

/// Initialises both the PICs and enables external interrupts.
pub(super) fn init() {
    unsafe {
        // Initalise ports
        writeb(Port::MainPicCmd, INIT_CMD);
        wait();
        writeb(Port::SecondaryPicCmd, INIT_CMD);
        wait();

        // Setup offsets
        writeb(Port::MainPicData, MAIN_OFFSET);
        wait();
        writeb(Port::SecondaryPicData, SECONDARY_OFFSET);
        wait();

        // Tell main about secondary
        writeb(Port::MainPicData, 4);
        wait();

        // Tell secondary how to forward to main
        writeb(Port::SecondaryPicData, 2);
        wait();

        // Use 8086 mode
        writeb(Port::MainPicData, MODE_8086);
        wait();
        writeb(Port::SecondaryPicData, MODE_8086);

        // Unmask
        writeb(Port::MainPicData, 0);
        writeb(Port::SecondaryPicData, 0);

        // Enable external interrupts
        core::arch::asm!("sti");
        vga::print_done("Initialised PICs");
    };
}

/// Waits a few microseconds by writing garbage data to port `0x80`.
fn wait() {
    unsafe {
        let unused_port = core::mem::transmute::<u16, Port>(0x80);
        writeb(unused_port, 0);
    }
}
