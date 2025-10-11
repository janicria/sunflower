use crate::wrappers::UnsafeFlag;
use core::fmt::Display;

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

/// Returns true if the GDT keyboard has been initialised.
pub fn gdt_init() -> bool {
    GDT_INIT.load()
}

/// Runs `task` as a startup task, printing `OK` or `ERR` depending on the result.
///
/// The task must **NEVER** assume interrupts to either be set or cleared when ran,
/// and must not rely on any kernel services which depend on their respective INIT static being set.
/// 
/// Aborts testing if tests are being ran and the task fails.
#[inline(never)]
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
    print!(fg = Lime, " OK");
    println!(" ] {task}");
}

/// Prints `[ ERR ] <task>: <err>`.
/// Fails the 'test' if tests are being ran.
fn print_err<E>(task: &str, err: E)
where
    E: Display,
{
    print!("[");
    print!(fg = LightRed, " ERR");
    println!(" ] {task}: {err}");

    #[cfg(test)]
    panic!("startup task {task} failed")
}
