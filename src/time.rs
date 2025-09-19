use crate::{
    wrappers::{InitError, InitLater},
    interrupts,
    ports::{self, Port},
    startup,
    vga::Corner,
};
use core::{
    arch::{asm, naked_asm},
    convert::Infallible,
    fmt::Display,
    hint, ptr,
    sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering},
};

/// The base frequency of the PIT.
pub static PIT_BASE_FREQ: u64 = 1193180;

/// Set when wait is called due to the crash when rebooting from another OS.
pub static WAITING: AtomicBool = AtomicBool::new(false);

/// The time the kernel was launched.
pub static LAUNCH_TIME: InitLater<Time> = InitLater::uninit();

/// Whether the time has been loaded into `LAUNCH_TIME` or not.
static RTC_SYNC_DONE: AtomicBool = AtomicBool::new(false);

/// CMOS register B.
static CMOS_REG_B: u8 = 0x8B;

/// Sets the timer interval in channel 0 to 10 ms.
pub fn set_timer_interval() -> Result<(), &'static str> {
    static MS_PER_TICK: u16 = 10;
    // divide by 1000 to convert from ms to seconds
    static TICK_INTERVAL: u16 = MS_PER_TICK * (PIT_BASE_FREQ / 1000) as u16;

    /// Binary mode, square wave, both lobyte & hibyte, channel 0
    ///
    /// [Reference](https://wiki.osdev.org/Programmable_Interval_Timer#I/O_Ports)
    static COMMAND: u8 = 0b0_111_11_00;

    if !startup::pic_init() {
        return Err("PIC is not initialised!!!");
    }

    interrupts::sti();

    // Safety: Sending valid command (see link above) 
    // and can assume the PIT was initialised after sending them
    unsafe {
        ports::writeb(Port::PITCmd, COMMAND);
        ports::writeb(Port::PITChannel0, TICK_INTERVAL as u8); // low byte
        ports::writeb(Port::PITChannel0, (TICK_INTERVAL >> 8) as u8); // high byte

        startup::PIT_INIT.store(true);
    }
    Ok(())
}

/// Returns how many ticks the kernel has been running for.
/// Increases every 10 ms or 100 Hz.
#[unsafe(naked)]
pub extern "C" fn get_time() -> u64 {
    /// The current time
    #[unsafe(no_mangle)]
    static mut TIME: u64 = 0;

    // Safety: I'm pretty both the increment and loading of TIME are only one instruction each
    naked_asm!("mov rax, [TIME]", "ret")
}

/// Toggles the waiting character on or off.
fn set_waiting_char(show: bool) {
    static PREV: AtomicU16 = AtomicU16::new(0);
    static WAITING_CHAR: u16 = 1025;
    let ptr = Corner::TopRight as usize as *mut u16;

    let write_waiting_char = |char: u16| {
        // Safety: TopRight is valid, aligned & won't do anything weird when written to
        unsafe {
            ptr::write_volatile(ptr, char);
        }
    };

    if show {
        // Safety: TopRight is valid, aligned & won't do anything weird when read from
        let prev = unsafe { ptr::read_volatile(ptr) };
        PREV.store(prev, Ordering::Relaxed);
        write_waiting_char(WAITING_CHAR);
    } else {
        // ptr = PREV
        let prev = PREV.load(Ordering::Relaxed);
        write_waiting_char(prev);
    }
}

/// Waits for `ticks` ticks (`ticks / 100` seconds).
///
/// Never returns if external interrupts are disabled.
pub fn wait(ticks: u64) {
    if !startup::pit_init() {
        return;
    }

    WAITING.store(true, Ordering::Relaxed);
    set_waiting_char(true);

    // wait...
    let target_time = get_time() + ticks;
    while get_time() < target_time {
        // Safety: Just halting
        unsafe { asm!("hlt") }
    }

    set_waiting_char(false);
    WAITING.store(false, Ordering::Relaxed);
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

    if !startup::pit_init() {
        return;
    }

    let target = TIME.load(Ordering::Relaxed) + ticks;
    set_waiting_char(true);

    // FIXME: What the hell is this
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

    set_waiting_char(false);
}

/// Second-precise time value.
pub struct Time {
    year: u8,
    month: u8,
    day: u8,
    hour: u8,
    min: u8,
    sec: u8,
}

impl Time {
    /// Returns the current time in the RTC.
    /// [`Reference`](https://wiki.osdev.org/CMOS#Getting_Current_Date_and_Time_from_RTC)
    fn now() -> Self {
        // Safety: Reading from valid registers.
        unsafe {
            Time {
                year: read_cmos_reg(0x9),
                month: read_cmos_reg(0x8),
                day: read_cmos_reg(0x7),
                hour: read_cmos_reg(0x4),
                min: read_cmos_reg(0x2),
                sec: read_cmos_reg(0x0),
            }
        }
    }
}

impl Display for Time {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            " {}:{}:{} {}/{}/{}",
            self.hour, self.min, self.sec, self.day, self.month, self.year
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

    // Set bit 6 in register B to enable interrupts.
    // Safety: Sending a valid command w/o external interrupts enabled
    unsafe {
        let prev = read_cmos_reg(CMOS_REG_B);
        ports::writeb(Port::CMOSSelector, CMOS_REG_B);
        ports::writeb(Port::CMOSRegister, prev | 0b1000000);
    }

    interrupts::sti();
    Ok(())
}

/// Waits for the RTC sync to finish then checks if `LAUNCH_TIME` has been successfully loaded.
pub fn wait_for_rtc_sync() -> Result<(), InitError<Time>> {
    // Wait until the RTC has been loaded into LAUNCH_TIME
    while !RTC_SYNC_DONE.load(Ordering::Relaxed) {
        hint::spin_loop(); // better performance via pause instruction
    }

    LAUNCH_TIME.read().map(|_| ())
}

/// Ran by RTC handler when the update ended interrupt occurs.
/// [`Reference`](https://wiki.osdev.org/CMOS#The_Real-Time_Clock)
#[unsafe(no_mangle)]
extern "C" fn sync_time_to_rtc() {
    /// The 24 hour time / 12 hour time flag in the hours value.
    static TWENTY_FOUR_HR_FLAG: u8 = 0b10000000;

    let mut time = Time::now();
    let reg_b = unsafe { read_cmos_reg(CMOS_REG_B) };
    let mut hour = time.hour;

    // If BCD mode (bit 2 clear), convert values to binary using the formula
    // Binary = ((BCD / 16) * 10) + (BCD & 0xF)
    if reg_b != reg_b | 0b100 {
        time.sec = bcd_to_bin(time.sec);
        time.min = bcd_to_bin(time.min);
        time.day = bcd_to_bin(time.day);
        time.month = bcd_to_bin(time.month);
        time.year = bcd_to_bin(time.year);

        // Preserve 24 hour flag
        hour = ((hour & 0x0F) + (((hour & 0x70) / 16) * 10)) | (hour & TWENTY_FOUR_HR_FLAG);
    }

    // If 12 hour time (bit 1 clear and flag set)
    if (reg_b != reg_b | 0b10) && (hour == hour & TWENTY_FOUR_HR_FLAG) {
        // Clear 24 / 12 hour flag and convert to 24 hour time
        time.hour = ((hour & 0b1111111) + 12) % 24;
    }

    // Ignore possible error as wait_for_rtc_sync checks this later
    _ = LAUNCH_TIME.init(time);
    RTC_SYNC_DONE.store(true, Ordering::Relaxed);

    fn bcd_to_bin(bcd: u8) -> u8 {
        ((bcd / 16) * 10) + (bcd & 0xF)
    }
}
