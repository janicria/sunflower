#![no_std]
#![no_main]

use core::panic::PanicInfo;

/// Allows writing to the VGA text buffer
#[macro_use]
mod vga;
/// Handles various interrupts
mod interrupts;
/// Handles writing and reading from specific I/O ports
mod ports;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    // Print welcome message
    use vga::Color;
    print!("Welcome to ");
    vga::print_color("Sunflower!\n", Color::LightCyan, Color::Black);

    interrupts::init();
    vga::print_color("All startup tasks completed! ", Color::Green, Color::Black);
    vga::print_color(str::from_utf8(&[1]).unwrap(), Color::Green, Color::Black); // happy face
    println!("\n");

    // unsafe { core::arch::asm!("int 42") } // gpf
    // interrupts::triple_fault();
    // unsafe { core::arch::asm!("int3") } // breakpoint
    // unsafe { core::arch::asm!("ud2")    // invalid op
    // unsafe { *(0x8 as *mut u64) = 42 }; // page fault

    idle()
}

/// Enters 'idle' mode.
fn idle() -> ! {
    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!(
        "PANIC OCCURRED: {}\nLocation: {}",
        info.message(),
        info.location().unwrap()
    );
    idle()
}
