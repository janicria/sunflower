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
    vga::print_color("Sunflower!\n", Color::LightCyan, Color::Black);
    
    interrupts::init();
    vga::print_color("All startup tasks completed\n\n", Color::Green, Color::Black);

    // None::<u8>.unwrap();
    // unsafe { core::arch::asm!("mov dx, 0; div dx") }

    loop {}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!(
        "PANIC OCCURED: {}\nLocation: {}",
        info.message(),
        info.location().unwrap()
    );
    loop {}
}
