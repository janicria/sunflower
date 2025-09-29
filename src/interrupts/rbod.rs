#[cfg(test)]
use crate::tests::exit_qemu;
use crate::{
    interrupts::{IntStackFrame, idt::ERR_CODE},
    ports::{self, Port},
    speaker,
    sysinfo::SystemInfo,
    time::{self, Time},
    vga::{
        buffers::{self, BUFFER_HEIGHT, BUFFER_WIDTH, YoinkedBuffer},
        print::{self, Color, Corner, VGAChar},
    },
};
use core::{
    panic::PanicInfo,
    ptr,
    sync::atomic::{AtomicU8, AtomicU16, AtomicU32, AtomicU64, Ordering},
};

/// Increased each time an exception with an `ErrorResponse::Continue` response occurs.
pub static SMALL_ERRS: AtomicU32 = AtomicU32::new(0);

/// Increased each time rbod is ran.
static BIG_ERRS: AtomicU32 = AtomicU32::new(0);
/// An error which caused handle_err to be run.
///
/// A lot of these errors should never actually occur,
/// and are just placed so the enum doesn't have any gaps.
#[derive(Debug, Copy, Clone, PartialEq)]
#[repr(u64)]
#[allow(dead_code)]
pub enum ErrorCode {
    DivisionError,
    Debug,
    NMIInterrupt,
    Breakpoint,
    Overflow,
    BoundRangeExceeded,
    InvalidOpcode,
    DeviceNotAvailable,
    DoubleFault,
    CoprocessorSegmentOverrun,
    GeneralProtectionFault = 13,
    PageFault,
    Invalid = 256,
    KernelPanic,
    SysCmd4,
}

/// Calls rbod, taking the `ERR_CODE` static as it's first argument.
#[unsafe(no_mangle)]
unsafe fn setup_rbod(frame: IntStackFrame) -> ! {
    unsafe { rbod(ERR_CODE, RbodErrInfo::Exception(frame), None) }
}

/// The error info passed to rbod by handle_err & panic.
pub enum RbodErrInfo<'a> {
    Exception(IntStackFrame),
    Panic(&'a PanicInfo<'a>),
    None,
}

/// Handler for exceptions which come with error codes.
pub struct ErrCodeHandler {
    handler: fn(u64),
    err_code: u64,
}

impl ErrCodeHandler {
    /// Creates a new handler to be passed to rbod.
    pub fn new(handler: fn(u64), err_code: u64) -> Option<Self> {
        Some(ErrCodeHandler { handler, err_code })
    }
}

/// Rainbow box of death. Very original name I know.
#[allow(clippy::unnecessary_cast)]
pub fn rbod(err: ErrorCode, info: RbodErrInfo, err_handler: Option<ErrCodeHandler>) -> ! {
    // Go into Uh-oh mode
    super::cli();
    BIG_ERRS.fetch_add(1, Ordering::Relaxed);
    time::set_waiting_char(false);
    time::WAITING_CHAR.store(false, Ordering::Relaxed);
    speaker::stop(); // in case anything was playing, prevent it from playing forever

    // Safety: Whatever was using the buffer will never be returned to from rbod
    unsafe { buffers::BUFFER_HELD.store(false) };
    buffers::swap();
    buffers::clear();

    // Begin the printing
    print::write_char(VGAChar::TOPLEFT_CORNER, Color::Grey, Color::Black);
    print!("------------------------------------------------------------------------------");
    print::write_char(VGAChar::TOPRIGHT_CORNER, Color::Grey, Color::Black);
    print!(
        fg = LightRed,
        "\n                An unrecoverable error has occurred: "
    );
    println!("{err:?}\n\n\n                                  ERROR INFO");

    // Print either the exception, panic or syscmd info
    match info {
        RbodErrInfo::Exception(frame) => {
            println!("{frame}")
        }
        RbodErrInfo::Panic(info) => {
            println!(
                "  Location: {}\n  Cause: {}",
                info.location().unwrap(), // always succeeds
                info.message()
            );
        }
        RbodErrInfo::None => {
            println!("             Caused by running either Ctrl+Alt+F4 or SysRq+F4\n")
        }
    }

    // Run the error code handler
    println!("\n                                ADDITIONAL INFO  ");
    if let Some(handler) = err_handler {
        (handler.handler)(handler.err_code)
    } else {
        println!("                          Not present for this error\n\n")
    }

    // Print some system info
    let sysinfo = SystemInfo::now();
    println!(
            "                                  SYSTEM INFO\n  Kernel: {}   CPU Vendor: {}   Debug: {}   
  Date: {}   Uptime: {}   Small errors: {}   Big errors: {}",
            sysinfo.sfk_version_long,
            sysinfo.cpu_vendor,
            sysinfo.debug,
            sysinfo.date.unwrap_or(&Time::default()),
            sysinfo.time,
            SMALL_ERRS.load(Ordering::Relaxed),
            BIG_ERRS.load(Ordering::Relaxed),
        );

    // Print the key press options
    print!(
        fg = LightBlue,
        "\n\n                        Press 1 to restart device
                        Press 2 to show previous output
                        Press 3 to play a relaxing song",
    );
    println!(
        fg = LightRed,
        "\n\n                             PRESS KEY TO PROCEED\n"
    );

    print::write_char(VGAChar::BOTTOMLEFT_CORNER, Color::Grey, Color::Black);
    print!("------------------------------------------------------------------------------");

    // Set the last
    unsafe {
        buffers::BUFFER[BUFFER_HEIGHT as usize - 1][BUFFER_WIDTH as usize - 1] =
            VGAChar::BOTTOMRIGHT_CORNER
    }

    // Always succeeds
    if let Some(mut buf) = YoinkedBuffer::try_yoink() {
        let buf = buf.buffer();

        // Set bottom right corner
        buf[BUFFER_HEIGHT as usize - 1][BUFFER_WIDTH as usize - 1] = VGAChar::BOTTOMRIGHT_CORNER;

        // Draw vertical lines for the box
        for row in 1..BUFFER_HEIGHT - 1 {
            buf[row as usize][0] = VGAChar::VERTICAL_BORDER;
            buf[row as usize][BUFFER_WIDTH as usize - 1] = VGAChar::VERTICAL_BORDER;
        }

        // Draw horizontal lines for the box
        for col in 1..BUFFER_WIDTH - 1 {
            buf[0][col as usize] = VGAChar::HORIZONTAL_BORDER;
            buf[BUFFER_HEIGHT as usize - 1][col as usize] = VGAChar::HORIZONTAL_BORDER;
        }
    }

    // loop forever...
    loop {
        time::wait_no_ints(20);
        check_keyboard();
        rbod_colors();
    }
}

/// Runs the corresponding action if any of the `Press KEY to X` keys are pressed
fn check_keyboard() {
    /// The last scancode read from port 0x60.
    static PREV_SCANCODE: AtomicU8 = AtomicU8::new(0);

    // Scancodes in set 2.
    static ONE: u8 = 0x16;
    static ONE_KEYPAD: u8 = 0x69;
    static TWO: u8 = 0x1E;
    static TWO_KEYPAD: u8 = 0x72;
    static THREE: u8 = 0x26;
    static THREE_KEYPAD: u8 = 0x7A;

    // Return if the previous scancode is the same as the current.
    let scancode = unsafe { ports::readb(Port::PS2Data) };
    if PREV_SCANCODE.swap(scancode, Ordering::Relaxed) == scancode {
        return;
    }

    // Run corresponding action
    if scancode == ONE || scancode == ONE_KEYPAD {
        super::triple_fault();
    } else if scancode == TWO || scancode == TWO_KEYPAD {
        buffers::swap()
    } else if scancode == THREE || scancode == THREE_KEYPAD {
        speaker::play_song();
    }
}

/// Changes the boxes colors.
fn rbod_colors() {
    /// The size of a `VgaChar`.
    static VGA_CHAR_SIZE: u64 = size_of::<VGAChar>() as u64;

    /// Grey and LightGrey.
    static SKIPPED_COLORS: [u16; 2] = [7, 8];

    /// The pointer to the last turn the rainbow made.
    static LAST_TURN: AtomicU64 = AtomicU64::new(Corner::TopLeft as u64);

    /// The current color.
    static COLOR: AtomicU16 = AtomicU16::new(Color::Red as u16);

    /// A pointer to the current char being colored.
    static CHAR_PTR: AtomicU64 = AtomicU64::new(Corner::TopLeft as u64);

    /// How much `CHAR_PTR` has to increase / decrease by, to move up / down one column.
    static VERTICAL_INCREASE: u64 = BUFFER_WIDTH as u64 * VGA_CHAR_SIZE;

    let char_ptr = CHAR_PTR.load(Ordering::Relaxed) as *mut u16;
    let color_bits = COLOR.load(Ordering::Relaxed) << 8;
    // Safety: TopRight is safe to write to and read from and won't do anything strange
    unsafe {
        // Set the current chars color to the COLOR static
        let char = ptr::read_volatile(char_ptr);
        ptr::write_volatile(char_ptr, char & 0b11111111 | color_bits);
    }

    // Update front char
    match LAST_TURN.load(Ordering::Relaxed) {
        v if v == Corner::TopLeft as u64 => {
            // Going to the right
            CHAR_PTR.fetch_add(VGA_CHAR_SIZE, Ordering::Relaxed);

            // If we've hit the top right corner
            if CHAR_PTR.load(Ordering::Relaxed) == Corner::TopRight as u64 + VGA_CHAR_SIZE {
                LAST_TURN.store(Corner::TopRight as u64, Ordering::Relaxed);
                CHAR_PTR.fetch_sub(VGA_CHAR_SIZE, Ordering::Relaxed);
            }
        }
        v if v == Corner::TopRight as u64 => {
            // Going down
            CHAR_PTR.fetch_add(VERTICAL_INCREASE, Ordering::Relaxed);

            // If we've hit the bottom right corner
            if CHAR_PTR.load(Ordering::Relaxed) == Corner::BottomRight as u64 {
                LAST_TURN.store(Corner::BottomRight as u64, Ordering::Relaxed);
                CHAR_PTR.fetch_sub(VERTICAL_INCREASE, Ordering::Relaxed);
            }
        }
        v if v == Corner::BottomRight as u64 => {
            // Going to the left
            CHAR_PTR.fetch_sub(VGA_CHAR_SIZE, Ordering::Relaxed);

            // If we've hit the bottom left corner
            if CHAR_PTR.load(Ordering::Relaxed) == Corner::BottomLeft as u64 {
                LAST_TURN.store(Corner::BottomLeft as u64, Ordering::Relaxed);
                CHAR_PTR.fetch_add(VGA_CHAR_SIZE, Ordering::Relaxed);
            }
        }
        v if v == Corner::BottomLeft as u64 => {
            // Going up
            CHAR_PTR.fetch_sub(VERTICAL_INCREASE, Ordering::Relaxed);

            // If we've completed a full box
            if CHAR_PTR.load(Ordering::Relaxed) == Corner::TopLeft as u64 {
                LAST_TURN.store(Corner::TopLeft as u64, Ordering::Relaxed);
                let color = COLOR.fetch_add(1, Ordering::Relaxed) + 1;

                // Wrap around when reaching the max
                if color > Color::Yellow as u16 {
                    COLOR.store(1, Ordering::Relaxed);
                };

                // Skip over colors marked as skipped
                while SKIPPED_COLORS.contains(&COLOR.load(Ordering::Relaxed)) {
                    COLOR.fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        _ => (),
    }
}

/// Ran when a panic occurs.
#[panic_handler]
#[allow(unreachable_code)]
fn panic(info: &PanicInfo) -> ! {
    #[cfg(test)]
    {
        println!("- failed, see failure cause below\n{info}");
        exit_qemu(true);
    }

    super::cli();
    rbod(ErrorCode::KernelPanic, RbodErrInfo::Panic(info), None)
}
