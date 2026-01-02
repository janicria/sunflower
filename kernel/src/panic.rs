//! Handles kernel panics via the [`panic!`] macro.

use crate::{
    floppy::motor,
    interrupts,
    ports::{self, Port},
    speaker,
    sysinfo::SystemInfo,
    time,
    vga::{
        buffers::{self, YoinkedBuffer},
        cursor::{self, CursorPos},
        print::Color,
    },
};
use core::{
    hint,
    panic::{Location, PanicInfo},
    sync::atomic::{AtomicBool, Ordering},
};

/// Ran when a kernel panic occurs.
#[panic_handler]
#[cfg_attr(test, allow(unused))]
fn kernel_panic(info: &PanicInfo) -> ! {
    #[cfg(test)]
    {
        // tests fail by panicking
        use crate::tests::exit_qemu;
        println!("- failed, see failure cause below\n{info}");
        exit_qemu(true);
    }

    // !!!!!!!!!
    interrupts::cli();
    time::set_waiting_char(false);
    time::WAITING_CHAR.store(false, Ordering::Relaxed);
    speaker::stop(); // in case anything was playing, prevent it from playing forever
    motor::force_disable(); // in case it was on
    // Safety: Whoever was using the buffer is long gone now
    unsafe { buffers::BUFFER_HELD.store(false) };

    // Swap & wipe screen
    cursor::ALLOW_ROW_0.store(true, Ordering::Relaxed);
    buffers::swap();
    buffers::clear();

    let location = info.location().unwrap(); // always succeeds
    let sysinfo = SystemInfo::now();

    println!(
        fg = Grey,
        "\n                                  KERNEL PANIC\n\n
      Sunflower encountered a kernel panic at {}:{}:{}\n
      System information: {} | {} | {} | {} | {}\n
      Press ESC to restart device and ENTER to show previous screen\n
                           Press any key to continue\n\n
      Error information:\n
      {}",
        location.file().trim_prefix("src/"),
        location.line(),
        location.column(),
        sysinfo.sfk_version_short,
        sysinfo.cpu_vendor,
        sysinfo.debug as u8,
        sysinfo.time,
        sysinfo.floppy_space.copied().unwrap_or_default(),
        info.message()
    );

    paint_screen();
    loop {
        check_keyboard(location);
        hint::spin_loop(); // can't halt because of cli
    }
}

/// Paints the screen's background color.
fn paint_screen() {
    /// The background color to paint the screen.
    const PANIC_COLOR: Color = Color::Red;

    // Yoink always succeeds
    if let Some(mut buf) = YoinkedBuffer::try_yoink() {
        for row in buf.buffer().iter_mut() {
            for px in row {
                let px = px.as_raw_mut();
                *px &= !(0b1111 << 12); // clear bg
                *px |= (PANIC_COLOR as u16) << 12; // set bg
            }
        }
    }

    // Move cursor to end of press any key line
    CursorPos::set_row(11);
    CursorPos::set_col(52);
    cursor::update_visual_pos();
}

/// Triple faults if `ESC` is pressed & prints sysinfo if `ALT` is pressed.
fn check_keyboard(location: &Location) {
    /// Should we allow swapping the buffers?
    static ALLOW_BUFSWAP: AtomicBool = AtomicBool::new(true);

    /// Scancodes in set 2.
    const ESC_SCANCODE: u8 = 0x76;
    const ENTER_SCANCODE: u8 = 0x5A;

    // Safety: Port 0x60 is fine to read as it just contains the last scancode.
    let scancode = unsafe { ports::readb(Port::PS2Data) };
    if scancode == ESC_SCANCODE {
        interrupts::triple_fault();
    } else if scancode == ENTER_SCANCODE && ALLOW_BUFSWAP.fetch_and(false, Ordering::Relaxed) {
        buffers::swap();
        CursorPos::set_row(u8::MAX);
        CursorPos::set_col(0);
        cursor::update_visual_pos();
        print!(
            fg = Grey,
            "-------------------------------------------------------------------------------- {} panicked at {}:{}:{} | Press ESC to restart",
            env!("SFK_VERSION_SHORT"),
            location.file().trim_prefix("src/"),
            location.line(),
            location.column()
        );
        cursor::update_visual_pos();
    }
}
