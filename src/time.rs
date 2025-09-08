use crate::{
    ports::{self, Port},
    vga::{self, Corner},
};
use core::{
    arch::asm,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

/// How many ticks the kernel has been running for.
/// Increases every 10 ms or 100 Hz.
#[unsafe(no_mangle)]
pub static mut TIME: u64 = 0;

/// The base frequency of the PIT.
pub static PIT_BASE_FREQ: u64 = 1193180;

/// Set when wait is called due to the crash when rebooting from another OS.
pub static WAITING: AtomicBool = AtomicBool::new(false);

/// Sets the timer interval in channel 0 to 10 ms.
pub fn set_timer_interval() {
    static MS_PER_TICK: u16 = 10;
    // divide by 1000 to convert from ms to seconds
    static TICK_INTERVAL: u16 = MS_PER_TICK * (PIT_BASE_FREQ / 1000) as u16;

    /// Binary mode, square wave, both lobyte & hibyte, channel 0
    ///
    /// [Reference](https://wiki.osdev.org/Programmable_Interval_Timer#I/O_Ports)
    static COMMAND: u8 = 0b0_111_11_00;

    unsafe {
        ports::writeb(Port::PITCmd, COMMAND);
        ports::writeb(Port::PITChannel0, TICK_INTERVAL as u8); // low byte
        ports::writeb(Port::PITChannel0, (TICK_INTERVAL >> 8) as u8); // high byte
    }

    vga::print_done("Initialised PIT");
}

/// Waits for approximately `ticks` ticks (`ticks / 100` seconds).
///
/// May be a few milliseconds shorter in times less than few seconds
/// and about 0.011% slower with times more than a few minutes.
///
/// Works with external interrupts disabled.
pub fn wait_no_ints(ticks: u64) {
    /// Channel 0. [Reference](https://wiki.osdev.org/Programmable_Interval_Timer#Counter_Latch_Command)
    static COMMAND: u8 = 0b00_000000;

    /// The lowest possible value the count can be before being reset.
    static MIN_COUNT_VALUE: u16 = 2;

    /// How many ticks have passed since the function was called.
    static TIME: AtomicU64 = AtomicU64::new(0);

    let target = TIME.load(Ordering::Relaxed) + ticks;
    while TIME.load(Ordering::Relaxed) < target {
        unsafe {
            ports::writeb(Port::PITCmd, COMMAND);
            let mut count = ports::readb(Port::PITChannel0) as u16; // low byte
            count |= (ports::readb(Port::PITChannel0) as u16) << 8; // high byte
            if count == MIN_COUNT_VALUE {
                TIME.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// Waits for `ticks` ticks (`ticks / 100` seconds).
///
/// Never returns if external interrupts are disabled.
pub fn wait(ticks: u64) {
    unsafe {
        static RED_SMILEY: u16 = 1025;
        WAITING.store(true, Ordering::Relaxed);

        // set waiting char
        let char = Corner::TopRight as usize as *mut u16;
        let prev = *char;
        *char = RED_SMILEY;

        // wait...
        let target_time = TIME + ticks;
        while TIME < target_time {
            asm!("hlt")
        }

        *char = prev;
        WAITING.store(false, Ordering::Relaxed);
    }
}
