use crate::ports::{self, Port};

/// Whether or not the speaker is repeatedly playing a sound.
/// This is different to if the speaker is holding a sound.
pub static mut REPEATING: bool = false;

/// Plays a sound with frequency `freq` using the pc speaker.
/// Plays the sound continuously if `hold` is set, otherwise repeats between on and off if not.
///
/// Note: QEMU requires extra support to emulate playing sounds through the pc speaker.
pub fn play(freq: u32, hold: bool) {
    unsafe {
        // Set the second channel in the PIT to freq
        let freq = 1193180 / freq;
        ports::writeb(Port::PITCmd, 0b10110110);
        ports::writeb(Port::PITChannel2, freq as u8);
        ports::writeb(Port::PITChannel2, (freq >> 8) as u8);
        REPEATING = !hold;

        // If the sound is low make it high
        let sound = ports::readb(Port::Speaker);
        if sound != sound | 3 {
            ports::writeb(Port::Speaker, sound | 3);
        }
    }
}

/// Plays `freq`, waits `time` ticks then stops.
pub fn play_duration(freq: u32, time: u64, hold: bool) {
    play(freq, hold);
    crate::wait(time);
    stop();
}

/// Plays the boot chime
pub fn play_chime() {
    play_duration(760, 8, true);
    crate::wait(6);

    play_duration(630, 4, true);
    play_duration(530, 6, true);

    play_duration(630, 4, true);
    play_duration(530, 6, true);

    play_duration(630, 8, true);
    play_duration(800, 10, true);
}

/// Stops the current sound the pc speaker is playing.
pub fn stop() {
    unsafe {
        // Make the sound low
        let sound = ports::readb(Port::Speaker) & 0b11111100;
        ports::writeb(Port::Speaker, sound);
        REPEATING = false;
    }
}

/// Ran when certain interrupts occur
#[unsafe(no_mangle)]
extern "C" fn play_error(extrabad: bool) {
    if extrabad {
        play_duration(600, 5, true);
    } else {
        play_duration(300, 4, true);
    }
}
