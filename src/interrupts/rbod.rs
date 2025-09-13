use crate::{
    interrupts::{IntStackFrame, idt::ERR_CODE},
    ports::{self, Port},
    speaker,
    sysinfo::SystemInfo,
    time,
    vga::{
        self, BUFFER, BUFFER_HEIGHT, BUFFER_WIDTH, Color, Corner, RawBuffer, VGAChar, YoinkedBuffer,
    },
};
use core::{
    mem,
    panic::PanicInfo,
    ptr,
    sync::atomic::{AtomicU8, AtomicU32, Ordering},
};

/// Increased each time an exception with an `ErrorResponse::Continue` response occurs.
pub static SMALL_ERRS: AtomicU32 = AtomicU32::new(0);

/// Increased each time rbod is ran.
static BIG_ERRS: AtomicU32 = AtomicU32::new(0);

/// The vga text buffer before the error
static mut PREV_VGA: RawBuffer = YoinkedBuffer::empty_buffer();

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
    swap_buffers();
    vga::clear();
    speaker::stop(); // in case anything was playing, prevent it from playing forever

    // Begin the printing
    println!("--------------------------------------------------------------------------------");
    vga::print_color(
        "                An unrecoverable error has occurred: ",
        Color::LightRed,
    );
    print!("{err:?}\n\n\n                                  ERROR INFO\n");

    // Print either the exception, panic or syscmd info
    match info {
        RbodErrInfo::Exception(frame) => {
            println!(
                "  Location: {:x}   Flags: {}   Code segment: {}\n  Stack pointer: {}   Stack segment: {} <- Should be zero",
                frame.ip, frame.flags, frame.cs, frame.sp, frame.ss
            )
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

    // Print the kernel info
    let sysinfo = SystemInfo::now();
    println!(
            "                                  SYSTEM INFO\n  Kernel: {}   CPU Vendor: {}   Debug: {}   
  Uptime: {}   Small errors: {}   Big errors: {}   Waiting: {}",
            sysinfo.sunflower_version,
            sysinfo.cpu_vendor,
            sysinfo.debug,
            sysinfo.time,
            SMALL_ERRS.load(Ordering::Relaxed),
            BIG_ERRS.load(Ordering::Relaxed),
            sysinfo.waiting
        );

    // Print the key press options
    vga::print_color(
        "\n\n                        Press 1 to restart device
                        Press 2 to show previous output
                        Press 3 to play a relaxing song",
        Color::LightBlue,
    );
    vga::print_color(
        "\n\n                             PRESS KEY TO PROCEED",
        Color::LightRed,
    );
    print!("\n\n-------------------------------------------------------------------------------");

    // Draw vertical lines for the box
    unsafe {
        for row in 0..BUFFER_HEIGHT {
            static PIPE: VGAChar = VGAChar::new(124, Color::White, Color::Black); // |
            vga::BUFFER[row][0] = PIPE;
            vga::BUFFER[row][BUFFER_WIDTH - 1] = PIPE;
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
        swap_buffers()
    } else if scancode == THREE || scancode == THREE_KEYPAD {
        speaker::play_song();
    }
}

/// Swaps the values of the `BUFFER` and `PREV_VGA` statics.
fn swap_buffers() {
    unsafe { ptr::swap(BUFFER, &raw mut PREV_VGA) }
}

/// Changes the boxes colors.
fn rbod_colors() {
    static VGA_CHAR: usize = size_of::<VGAChar>();
    static SKIPPED_COLORS: [Color; 2] = [Color::Grey, Color::LightGrey];
    static mut LAST_TURN: Corner = Corner::TopLeft; // the last turn the rainbow made
    static mut COLOR: u16 = Color::Red as u16; // current color
    static mut FRONT_CHAR: usize = Corner::TopLeft as usize; // current char

    // Set chars color to color
    unsafe {
        let char = FRONT_CHAR as *mut u16;
        *char = *char & 0b00000000_11111111 | COLOR << 8;
    }

    // Update front char
    match unsafe { LAST_TURN } {
        Corner::TopLeft => unsafe {
            // going to the right
            FRONT_CHAR += VGA_CHAR;

            if FRONT_CHAR == Corner::TopRight as usize + VGA_CHAR {
                LAST_TURN = Corner::TopRight;
                FRONT_CHAR -= VGA_CHAR
            }
        },
        Corner::TopRight => unsafe {
            // going down
            FRONT_CHAR += (BUFFER_WIDTH) * VGA_CHAR;

            if FRONT_CHAR == Corner::BottomRight as usize {
                LAST_TURN = Corner::BottomRight;
                FRONT_CHAR -= (BUFFER_WIDTH) * VGA_CHAR;
            }
        },
        Corner::BottomRight => unsafe {
            // going to the left
            FRONT_CHAR -= VGA_CHAR;

            if FRONT_CHAR == Corner::BottomLeft as usize {
                LAST_TURN = Corner::BottomLeft;
                FRONT_CHAR += VGA_CHAR;
            }
        },
        Corner::BottomLeft => unsafe {
            // going up
            FRONT_CHAR -= BUFFER_WIDTH * VGA_CHAR;

            if FRONT_CHAR == Corner::TopLeft as usize {
                LAST_TURN = Corner::TopLeft;

                // Increase color
                COLOR += 1;
                if COLOR > Color::Yellow as u16 {
                    COLOR = 1
                };
                while SKIPPED_COLORS.contains(&mem::transmute::<u16, Color>(COLOR)) {
                    COLOR += 1;
                }
            }
        },
    }
}

/// Ran when a panic occurs.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    super::cli();
    rbod(ErrorCode::KernelPanic, RbodErrInfo::Panic(info), None)
}
