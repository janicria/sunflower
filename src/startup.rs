use crate::vga::{self, Color};
use core::{
    fmt::Display,
    sync::atomic::{AtomicBool, Ordering},
};

/// Whether or not the kernel has finished all it's startup tasks.  
pub static SYS_INIT: AtomicBool = AtomicBool::new(false);

/// Returns true if the kernel has finished all it's startup tasks.
pub fn init() -> bool {
    SYS_INIT.load(Ordering::Relaxed)
}

/// Runs `task` as a startup task, printing `OK` or `ERR` depending on the result.
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
