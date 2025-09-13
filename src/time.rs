use crate::{
    interrupts,
    ports::{self, Port},
    startup,
    vga::Corner,
};
use core::{
    arch::asm,
    convert::Infallible,
    fmt::Display,
    mem,
    sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
};

/// How many ticks the kernel has been running for.
/// Increases every 10 ms or 100 Hz.
#[unsafe(no_mangle)]
pub static mut TIME: u64 = 0;

/// The base frequency of the PIT.
pub static PIT_BASE_FREQ: u64 = 1193180;

/// Set when wait is called due to the crash when rebooting from another OS.
pub static WAITING: AtomicBool = AtomicBool::new(false);

/// The time the kernel was launched.
pub static LAUNCH_TIME: Time = unsafe { mem::zeroed() };

/// CMOS register B.
static CMOS_REG_B: u8 = 0x8B;

/// Sets the timer interval in channel 0 to 10 ms.
pub fn set_timer_interval() -> Result<(), Infallible> {
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

    Ok(())
}

/// Waits for approximately `ticks` ticks (`ticks / 100` seconds).
///
/// May be a few milliseconds shorter in times less than few seconds in a VM.
/// And A LOT slower on ancient computers.
///
/// Works with external interrupts disabled.
pub fn wait_no_ints(ticks: u64) {
    /// Channel 0. [Reference](https://wiki.osdev.org/Programmable_Interval_Timer#Counter_Latch_Command)
    static COMMAND: u8 = 0b00_000000;

    /// The lowest possible value the count can be before being reset.
    static MIN_COUNT_VALUE: u16 = 2;

    /// How many ticks have passed since the function was called.
    static TIME: AtomicU64 = AtomicU64::new(0);

    if !startup::init() {
        return;
    }

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
    if !startup::init() {
        return;
    }

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

/// Second-precise time value.
pub struct Time {
    year: AtomicU8,
    month: AtomicU8,
    day: AtomicU8,
    hour: AtomicU8,
    min: AtomicU8,
    sec: AtomicU8,
}

impl Time {
    /// Returns the current time in the RTC.
    /// [`Reference`](https://wiki.osdev.org/CMOS#Getting_Current_Date_and_Time_from_RTC)
    fn now() -> Self {
        unsafe {
            Time {
                year: AtomicU8::new(read_cmos_reg(0x9)),
                month: AtomicU8::new(read_cmos_reg(0x8)),
                day: AtomicU8::new(read_cmos_reg(0x7)),
                hour: AtomicU8::new(read_cmos_reg(0x4)),
                min: AtomicU8::new(read_cmos_reg(0x2)),
                sec: AtomicU8::new(read_cmos_reg(0x0)),
            }
        }
    }
}

impl Display for Time {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            " {}:{}:{} {}/{}/{}",
            self.hour.load(Ordering::Relaxed),
            self.min.load(Ordering::Relaxed),
            self.sec.load(Ordering::Relaxed),
            self.day.load(Ordering::Relaxed),
            self.month.load(Ordering::Relaxed),
            self.year.load(Ordering::Relaxed),
        )
    }
}

/// Returns the current value of CMOS register `reg`.
/// # Safety
/// Reads and writes to I/O ports.
unsafe fn read_cmos_reg(reg: u8) -> u8 {
    unsafe {
        ports::writeb(Port::CMOSSelector, reg);
        ports::readb(Port::CMOSRegister)
    }
}

/// Sets up RTC interrupts in IRQ 8.
pub fn setup_rtc_int() -> Result<(), Infallible> {
    interrupts::cli();

    // Set bit 6 in register B.
    unsafe {
        let prev = read_cmos_reg(CMOS_REG_B);
        ports::writeb(Port::CMOSSelector, CMOS_REG_B);
        ports::writeb(Port::CMOSRegister, prev | 0b1000000);
    }

    interrupts::sti();
    Ok(())
}

/// Ran by RTC handler when the update ended interrupt occurs.
/// [`Reference`](https://wiki.osdev.org/CMOS#The_Real-Time_Clock)
#[unsafe(no_mangle)]
extern "C" fn sync_time_to_rtc() {
    /// The 24 hour / AM PM flag in the hours value.
    static TWENTY_FOUR_HR_FLAG: u8 = 0b10000000;

    let time = Time::now();
    let reg_b = unsafe { read_cmos_reg(CMOS_REG_B) };
    let mut hour = time.hour.load(Ordering::Relaxed);

    // If BCD mode (bit 2 clear), convert values to binary using the formula
    // Binary = ((BCD / 16) * 10) + (BCD & 0xF)
    if reg_b != reg_b | 0b100 {
        bcd_to_bin(&time.sec);
        bcd_to_bin(&time.min);
        bcd_to_bin(&time.day);
        bcd_to_bin(&time.month);
        bcd_to_bin(&time.year);

        // Preserve 24 hour flag
        hour = ((hour & 0x0F) + (((hour & 0x70) / 16) * 10)) | (hour & TWENTY_FOUR_HR_FLAG);
    }

    // If 12 hour time (bit 1 clear and flag set)
    if (reg_b != reg_b | 0b10) && (hour == hour & TWENTY_FOUR_HR_FLAG) {
        let hour = ((hour & 0x7F) + 12) % 24;
        time.hour.store(hour, Ordering::Relaxed);
    }

    // Store time
    let relaxed = Ordering::Relaxed;
    LAUNCH_TIME.year.store(time.year.load(relaxed), relaxed);
    LAUNCH_TIME.month.store(time.month.load(relaxed), relaxed);
    LAUNCH_TIME.day.store(time.day.load(relaxed), relaxed);
    LAUNCH_TIME.min.store(time.min.load(relaxed), relaxed);
    LAUNCH_TIME.sec.store(time.sec.load(relaxed), relaxed);

    fn bcd_to_bin(val: &AtomicU8) {
        let bcd = val.load(Ordering::Relaxed);
        val.store(((bcd / 16) * 10) + (bcd & 0xF), Ordering::Relaxed)
    }
}
