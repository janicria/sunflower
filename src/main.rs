#![no_std]
#![no_main]

use core::panic::PanicInfo;

/// Allows writing to the VGA text buffer
#[macro_use]
mod vga;
/// 
mod interrupts;

#[allow(clippy::empty_loop)]
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    // Print welcome message
    use vga::Color;
    print!("Welcome to ");
    vga::WRITER.lock().write_str("Sunflower!\n\n", Color::LightCyan, Color::Black);

    loop {}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    loop {}
}