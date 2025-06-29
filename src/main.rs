#![no_std]
#![no_main]

use core::panic::PanicInfo;

/// Allows writing to the VGA text buffer
#[macro_use]
mod vga;
/// Handles various interrupts
mod interrupts;
/// Handles writing to and reading from specific I/O ports
mod ports;
/// Allows playing sounds through the PC speaker
mod speaker;

/// How many ticks the kernel has been running for.
/// Increases every ~18.2 Hz
static mut TIME: u64 = 0;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    vga::init();
    interrupts::init();

    use vga::Color;
    vga::print_color("All startup tasks completed! ", Color::Green, Color::Black);
    vga::print_color(str::from_utf8(&[1]).unwrap(), Color::Green, Color::Black); // happy face
    println!("\n");
    speaker::play_chime();

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

/// Waits for `ticks` ticks.
fn wait(ticks: u64) {
    unsafe {
        let target_time = TIME + ticks;
        while TIME < target_time {
            core::arch::asm!("sti;hlt;cli");
        }
        core::arch::asm!("sti")
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
