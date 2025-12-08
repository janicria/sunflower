use core::fmt::Display;
use libutil::UnsafeFlag;

use crate::vga::print::{self, Color};

// Whether or not the GDT has been initialised yet
/// # Flag
/// Falsely setting this flag to true causes the TSS keyboard assume it's ready to be initialised.
pub static GDT_INIT: UnsafeFlag = UnsafeFlag::new(false);

/// Whether or not the PIC has been initialised yet
/// # Flag
/// Falsely setting this flag to true causes the PIT & PS/2 keyboard assume they're ready to be initialised.
pub static PIC_INIT: UnsafeFlag = UnsafeFlag::new(false);

/// Whether or not the PIT has been initialised yet
/// # Flag
/// Falsely setting this flag to true causes `time::wait` to loop forever and causes
/// `time::wait_no_ints` and `speaker::play` to assume that they've been initialised.
pub static PIT_INIT: UnsafeFlag = UnsafeFlag::new(false);

/// Whether or not the PS/2 keyboard has been initialised yet
/// # Flag
/// Setting this flag to true too early causes kbd_handler to break the keyboard init function.
pub static KBD_INIT: UnsafeFlag = UnsafeFlag::new(false);

/// Whether or not the floppy controller has been initialised yet.
/// # Flag
/// Falsely setting this flag to true causes services in `floppy::disk` to assume that they've been initialised.
pub static FLOPPY_INIT: UnsafeFlag = UnsafeFlag::new(false);

/// Has the Real Time Clock IRQ been initialised yet?
/// # Flag
/// Falsely setting this flag in startup causes [`wait_for_rtc_sync`](crate::time::wait_for_rtc_sync) to loop forever.
/// This isn't really unsafe, but it is very scary.
pub static RTC_IRQ_INIT: UnsafeFlag = UnsafeFlag::new(false);

/// Returns [`ExitCode`] `code` if `res` is `Err`.
#[macro_export]
macro_rules! exit_on_err {
    ($res: expr, $code: ident) => {
        match $res {
            Ok(val) => val,
            Err(e) => return $crate::startup::ExitCode::$code(e.into()),
        }
    };
    ($res: expr) => {
        $crate::exit_on_err!($res, Error)
    };
}

/// Runs  startup task `task`.
///
/// Aborts testing if tests are being ran and the task fails.
///
/// # Safety
/// The task must be safe to run, only be ran once, and be aware that
/// the kernel can be in any state when first ran (such as having interrupts clear).
pub unsafe fn run<E>(name: &str, task: unsafe fn() -> ExitCode<E>)
where
    E: Display,
{
    // Safety: The caller must ensure that the task is safe to run
    match unsafe { task() } {
        ExitCode::Infallible => print_box(Color::Cyan, "INF", name),
        ExitCode::Ok => print_box(Color::Lime, "OK!", name),
        ExitCode::Error(e) => {
            print_box(Color::LightRed, "ERR", name);
            println!(fg = LightGrey, "error: {e}");
        }
        ExitCode::Stop(e) => {
            print_box(Color::Red, "STP", name);
            println!("startup task encountered an unrecoverable error: {e}");
            panic!("startup task {name} returned STOP");
        }
    };

    fn print_box(fg: Color, code: &str, name: &str) {
        print::write_char(b'[', Color::White, Color::Black);
        print::_print(format_args!(" {code} "), fg, Color::Black);
        print::write_char(b']', Color::White, Color::Black);
        println!(fg = Grey, " {name}");
    }
}

/// An exit code returned from a startup task.
pub enum ExitCode<E> {
    /// The task can't fail.
    Infallible,

    /// The task passed, enable the group flags.
    Ok,

    /// The task encountered an error, disable the group flags.
    Error(E),

    /// Stop execution of the kernel and hang.
    Stop(E),
}
