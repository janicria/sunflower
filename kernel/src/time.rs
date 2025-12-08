use crate::{
    interrupts,
    ports::{self, Port},
    startup::{self, ExitCode},
    vga::print::{Color, Corner, VGAChar},
};
use core::{
    arch::{asm, naked_asm},
    fmt::Display,
    hint, ptr,
    sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering},
};
use libutil::InitLater;
use thiserror::Error;

/// The base frequency of the PIT.
pub static PIT_BASE_FREQ: u64 = 1193180;

/// The time the kernel was launched.
pub static LAUNCH_TIME: InitLater<Time> = InitLater::uninit();

/// Whether the time has been loaded into `LAUNCH_TIME` or not.
static RTC_SYNC_DONE: AtomicBool = AtomicBool::new(false);

/// CMOS register B.
static CMOS_REG_B: u8 = 0x8B;

/// The waiting character is only able to be toggled when this static is.
pub static WAITING_CHAR: AtomicBool = AtomicBool::new(true);

/// Sets the timer interval in channel 0 to 10 ms.
pub fn set_timer_interval() -> ExitCode<&'static str> {
    if !startup::PIC_INIT.load() {
        return ExitCode::Error("The PIC isn't init!");
    }

    static MS_PER_TICK: u16 = 10;
    // divide by 1000 to convert from ms to seconds
    static TICK_INTERVAL: u16 = MS_PER_TICK * (PIT_BASE_FREQ / 1000) as u16;

    /// Binary mode, square wave, both lobyte & hibyte, channel 0
    ///
    /// [Reference](https://wiki.osdev.org/Programmable_Interval_Timer#I/O_Ports)
    static COMMAND: u8 = 0b0_111_11_00;

    interrupts::sti();

    // Safety: Sending valid command (see link above)
    // and can assume the PIT was initialised after sending them
    unsafe {
        ports::writeb(Port::PITCmd, COMMAND);
        ports::writeb(Port::PITChannel0, TICK_INTERVAL as u8); // low byte
        ports::writeb(Port::PITChannel0, (TICK_INTERVAL >> 8) as u8); // high byte

        startup::PIT_INIT.store(true);
    }
    ExitCode::Infallible
}

/// Returns how many ticks the kernel has been running for.
/// Increases every 10 ms or 100 Hz.
#[unsafe(naked)]
pub extern "C" fn get_time() -> u64 {
    /// The current time
    #[unsafe(no_mangle)]
    static mut TIME: u64 = 0;

    // Safety: I'm pretty sure both the increment and loading of TIME are only one instruction each
    naked_asm!("mov rax, [TIME]", "ret")
}

/// Toggles the waiting character on or off.
pub fn set_waiting_char(show: bool) {
    if !WAITING_CHAR.load(Ordering::Relaxed) {
        return;
    }

    const CHAR: u16 = VGAChar::new(1, Color::Black, Color::LightGrey).as_u16();
    static PREV: AtomicU16 = AtomicU16::new(0);
    
    let ptr = Corner::TopRight as usize as *mut u16;
    let write_char = |char: u16| {
        // Safety: TopRight is valid, aligned & won't do anything weird when written to
        unsafe { ptr::write_volatile(ptr, char) }
    };

    if show {
        // Safety: TopRight is valid, aligned & won't do anything weird when read from
        let prev = unsafe { ptr::read_volatile(ptr) };
        PREV.store(prev, Ordering::Relaxed);
        write_char(CHAR);
    } else {
        let prev = PREV.load(Ordering::Relaxed);
        write_char(prev);
    }
}

/// Waits for `ticks` ticks (`ticks / 100` seconds).
///
/// Never returns if external interrupts are disabled.
pub fn wait(ticks: u64) {
    if !startup::PIT_INIT.load() {
        warn!("attempted waiting (with ints) with an uninit PIT!");
        return;
    }

    set_waiting_char(true);

    // wait...
    let target_time = get_time() + ticks + 1;
    while get_time() < target_time {
        // Safety: Just halting
        unsafe { asm!("hlt") }
    }

    set_waiting_char(false);
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

    if !startup::PIT_INIT.load() {
        warn!("attempted waiting (without ints) with an uninit PIT!");
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

/// Returns if a timer going for at least `timeout` ticks starting at `start` is still running.
pub fn timer(start: u64, timeout: u64) -> bool {
    start + timeout > get_time()
}

/// The century the kernel was complied.
/// This value is only updated when the kernel is built so isn't too precise.
const CENTURY: u16 = crate::env_as_int!("SFK_TIME_CENTURY", u16);

/// Second-precise time value.
#[derive(Debug, Default, Clone, Copy)]
pub struct Time {
    /// The current year, 0-65535
    year: u16,

    /// The current month, 1-12
    month: u8,

    /// The current day of the month, 1-31
    day: u8,

    /// The number of hours that have passed in the day, 0-23
    hour: u8,

    /// The number of minutes that have passed in the hour, 0-59
    min: u8,

    /// The number of seconds that have passed in the minute, 0-59
    sec: u8,
}

impl Time {
    /// Returns the current time in the RTC.
    /// [`Reference`](https://wiki.osdev.org/CMOS#Getting_Current_Date_and_Time_from_RTC)
    fn now() -> Self {
        // Safety: Reading from valid registers.
        unsafe {
            Time {
                year: read_cmos_reg(0x9) as u16,
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
            "{}:{}:{} {}/{}/{}",
            self.hour, self.min, self.sec, self.day, self.month, self.year
        )
    }
}

/// Returns the current value of CMOS register `reg`.
/// # Safety
/// Reads and writes to I/O ports.
pub unsafe fn read_cmos_reg(reg: u8) -> u8 {
    unsafe {
        ports::writeb(Port::CMOSSelector, reg);
        ports::readb(Port::CMOSRegister)
    }
}

/// Sets up RTC interrupts in IRQ 8.
pub fn setup_rtc_int() -> ExitCode<&'static str> {
    if !startup::PIC_INIT.load() {
        return ExitCode::Error("The PIC isn't init!");
    }

    interrupts::cli();

    // Set bit 6 in register B to enable interrupts.
    // Safety: Sending a valid command with external interrupts disabled
    unsafe {
        let prev = read_cmos_reg(CMOS_REG_B);
        ports::writeb(Port::CMOSSelector, CMOS_REG_B);
        ports::writeb(Port::CMOSRegister, prev | 0b1000000);
    }

    // Safety: Just enabled it above!
    unsafe { startup::RTC_IRQ_INIT.store(true) }

    interrupts::sti();
    ExitCode::Infallible
}

/// Waits for the RTC sync to finish then checks if `LAUNCH_TIME` has been successfully loaded.
pub fn wait_for_rtc_sync() -> ExitCode<RtcSyncWaitError> {
    if !startup::RTC_IRQ_INIT.load() {
        return ExitCode::Error(RtcSyncWaitError::NoIrq);
    }

    // Wait until the time has been loaded into LAUNCH_TIME
    while !RTC_SYNC_DONE.load(Ordering::Relaxed) {
        hint::spin_loop(); // better performance via pause instruction
    }

    if let Err(e) = LAUNCH_TIME.read() {
        return ExitCode::Error(RtcSyncWaitError::NoStatic(e.state));
    }

    ExitCode::Ok
}

#[derive(Error, Debug)]
pub enum RtcSyncWaitError {
    #[error("The RTC IRQ isn't enabled!")]
    NoIrq,

    #[error("The RTC handler failed updating the launch time, static's init state is {0}")]
    NoStatic(u8),
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
        time.year = bcd_to_bin(time.year as u8) as u16;

        // Preserve 24 hour flag
        hour = ((hour & 0x0F) + (((hour & 0x70) / 16) * 10)) | (hour & TWENTY_FOUR_HR_FLAG);
    }

    // If 12 hour time (bit 1 clear and flag set)
    if (reg_b != reg_b | 0b10) && (hour == hour & TWENTY_FOUR_HR_FLAG) {
        // Clear 24 / 12 hour flag and convert to 24 hour time
        time.hour = ((hour & 0b1111111) + 12) % 24;
    }

    // Add the century
    time.year += CENTURY * 100;

    // Ignore possible error as wait_for_rtc_sync checks this later
    _ = LAUNCH_TIME.init(time);
    RTC_SYNC_DONE.store(true, Ordering::Relaxed);

    fn bcd_to_bin(bcd: u8) -> u8 {
        ((bcd / 16) * 10) + (bcd & 0xF)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::speaker;

    /// Tests that `wait` waits for the correct amount of time.
    #[test_case]
    fn wait_waits_for_correct_time() {
        // Ensures that time doesn't increase in between getting time & starting waiting
        wait(1);

        let time = get_time();
        wait(15);
        assert!(get_time() - 15 - time < 3) // less than 3 tick difference
    }

    /// Tests that `wait`, `wait_no_ints` & `play_special` immediately return if the PIT failed initialisation.
    #[test_case]
    fn wait_services_require_pit() {
        let init = startup::PIT_INIT.load();
        unsafe { startup::PIT_INIT.store(false) }

        // Test fails due to timeout
        wait(u64::MAX);
        wait_no_ints(u64::MAX);
        speaker::play_special(0, u64::MAX, false, false);

        unsafe { startup::PIT_INIT.store(init) }
    }

    /// Tests that the RTC contains sane values through `LAUNCH_TIME`.
    #[test_case]
    fn rtc_contains_sane_values() {
        let time = LAUNCH_TIME.read().unwrap();
        assert!((time.year - CENTURY * 100) < 100);
        assert!(time.month != 0 && time.month <= 12);
        assert!(time.day != 0 && time.day <= 31);
        assert!(time.hour < 24);
        assert!(time.min < 60);
        assert!(time.sec < 60);
    }
}
