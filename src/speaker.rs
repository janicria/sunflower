use crate::ports::{self, Port};

/// Whether or not the speaker is repeatedly playing a sound.
/// This is different to if the speaker is holding a sound.
pub static mut REPEATING: bool = false;

/// Plays a sound with frequency `freq` using the pc speaker.
/// Plays the sound continuously if `hold` is set, otherwise repeats between on and off if not.
///
/// Note: QEMU requires extra support to emulate playing sounds through the pc speaker,
/// and may not be able to produce repeating frequencies when used with certain headphones
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

/// Holds `freq` for `time` ticks then stops.
pub fn hold_duration(freq: u32, time: u64) {
    play(freq, true);
    crate::wait(time);
    stop();
}

/// Repeats `freq` for `time` ticks then stops.
pub fn repeat_duration(freq: u32, time: u64) {
    play(freq, false);
    crate::wait(time);
    stop();
}

/// Plays the boot chime
pub fn play_chime() {
    hold_duration(600, 7);
    hold_duration(620, 9);

    hold_duration(600, 7);
    hold_duration(780, 20);
}

pub fn play_song() {
    // Set 1 - Rising
    hold_duration(400, 6);
    hold_duration(430, 6);
    hold_duration(450, 6);
    hold_duration(500, 5);
    hold_duration(550, 5);

    // Set 2 - Beeping 1
    hold_duration(450, 2);
    hold_duration(400, 3);
    hold_duration(500, 5);
    hold_duration(550, 5);

    // Set 3 - Beeping 2
    hold_duration(600, 2);
    hold_duration(620, 2);
    hold_duration(600, 2);
    hold_duration(620, 2);
    hold_duration(600, 2);
    hold_duration(620, 2);
    hold_duration(500, 2);
    hold_duration(480, 2);

    // Set 2 - Beeping 1
    hold_duration(450, 2);
    hold_duration(400, 3);
    hold_duration(500, 5);
    hold_duration(550, 5);

    // Set 1 - Rising
    hold_duration(400, 6);
    hold_duration(430, 6);
    hold_duration(450, 6);
    hold_duration(500, 5);
    hold_duration(550, 5);

    // Set 2 - Beeping 1
    hold_duration(450, 2);
    hold_duration(400, 3);
    hold_duration(500, 5);
    hold_duration(550, 5);

    // Set 4 - Uh oh
    repeat_duration(600, 14);
    hold_duration(500, 16);
    repeat_duration(600, 14);

    // Set 5 - Fade out
    hold_duration(550, 2);
    hold_duration(540, 2);
    hold_duration(530, 2);
    hold_duration(520, 2);
    hold_duration(510, 2);
    hold_duration(500, 2);
    hold_duration(490, 2);
    hold_duration(480, 2);
    hold_duration(470, 2);
    hold_duration(460, 2);
    hold_duration(450, 30);
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
