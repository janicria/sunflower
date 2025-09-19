use crate::{
    vga::{self, Color},
    wrappers::UnsafeFlag,
};
use core::fmt::Display;

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

/// Returns true if the PIC has been initialised.
pub fn pic_init() -> bool {
    PIC_INIT.load()
}

/// Returns true if the PIT has been initialised.
pub fn pit_init() -> bool {
    PIT_INIT.load()
}

/// Returns true if the PS/2 keyboard has been initialised.
pub fn kbd_init() -> bool {
    KBD_INIT.load()
}

/// Runs `task` as a startup task, printing `OK` or `ERR` depending on the result.
///
/// The task must **NEVER** assume interrupts to either be set or cleared when ran,
/// and must not rely on any kernel services which depend on their respective INIT static being set.
pub fn run<E>(name: &str, task: fn() -> Result<(), E>)
where
    E: Display,
{
    match task() {
        Ok(()) => print_ok(name),
        Err(s) => print_err(name, s),
    }
}

/// Prints `[ OK! ] <task>`.
pub fn print_ok(task: &str) {
    print!("[");
    vga::print_color(" OK", Color::Lime);
    println!(" ] {task}");
}

/// Prints `[ ERR ] <task>: <err>`.
fn print_err<E>(task: &str, err: E)
where
    E: Display,
{
    print!("[");
    vga::print_color(" ERR", Color::LightRed);
    println!(" ] {task}: {err}");
}
