use crate::{
    ports::{self, Port},
    time,
};

/// The bits required for the PC speaker to play sound through PIT channel 2.
static PLAY_BITS: u8 = 3;

/// Plays a sound with the specified frequency to the pc speaker.
///
/// Note: QEMU doesn't seem to be able to rapidly switch between playing different sounds
/// when used with certain headphones and requires passing
/// `-audio driver=<insert driver here>,model=virtio,id=speaker --machine pcspk-audiodev=speaker`
pub fn play(freq: u32) {
    unsafe {
        static COMMAND: u8 = 0b10110110;

        // Set the second channel in the PIT to freq
        let freq = time::PIT_BASE_FREQ as u32 / freq;
        ports::writeb(Port::PITCmd, COMMAND);
        ports::writeb(Port::PITChannel2, freq as u8); // low byte
        ports::writeb(Port::PITChannel2, (freq >> 8) as u8); // high byte

        // If the play bits are not set, enable them
        let val = ports::readb(Port::Speaker);
        if val != val | PLAY_BITS {
            ports::writeb(Port::Speaker, val | PLAY_BITS);
        }
    }
}

/// Stops the current sound the pc speaker is playing.
pub fn stop() {
    unsafe {
        // Disable the play bits
        let val = ports::readb(Port::Speaker) & !PLAY_BITS;
        ports::writeb(Port::Speaker, val);
    }
}

/// Plays `freq` for `time` milliseconds.
///
/// Repeatedly plays then stops playing at 100ms intervals if `repeat` is set.
///
/// Works without external interrupts, yet is slightly inaccurate if `no_ints` is set.
pub fn play_special(freq: u32, millis: u64, repeat: bool, no_ints: bool) {
    let (wait, ticks) = if no_ints {
        // Convert millis to double the usual ticks due to
        // wait_no_int playing much faster at shorter times.
        (time::wait_no_ints as fn(u64), millis / 5)
    } else {
        // Convert millis to ticks.
        (time::wait as fn(u64), millis / 10)
    };

    if repeat {
        static PULSE_LENGTH: u64 = 10;
        for _ in 0..ticks / (PULSE_LENGTH * 2) {
            play(freq);
            wait(PULSE_LENGTH);
            stop();
            wait(PULSE_LENGTH);
        }
    } else {
        play(freq);
        wait(ticks);
        stop();
    }
}

/// Plays the boot chime.
pub fn play_chime() {
    play_special(600, 350, false, false);
    play_special(620, 450, false, false);

    play_special(600, 350, false, false);
    play_special(780, 900, false, false);
}

/// Plays when an rbod has occurred and everything is wrong in the world.
pub fn play_song() {
    // Set 1 - Rising
    play_special(400, 300, false, true);
    play_special(430, 300, false, true);
    play_special(450, 300, false, true);
    play_special(500, 250, false, true);
    play_special(550, 250, false, true);

    // Set 2 - Beeping 1
    play_special(450, 100, false, true);
    play_special(400, 150, false, true);
    play_special(500, 250, false, true);
    play_special(550, 250, false, true);

    // Set 3 - Beeping 2
    play_special(600, 100, false, true);
    play_special(620, 100, false, true);
    play_special(600, 100, false, true);
    play_special(620, 100, false, true);
    play_special(600, 100, false, true);
    play_special(620, 100, false, true);
    play_special(500, 100, false, true);
    play_special(480, 100, false, true);

    // Set 2 - Beeping 1
    play_special(450, 100, false, true);
    play_special(400, 150, false, true);
    play_special(500, 250, false, true);
    play_special(550, 250, false, true);

    // Set 1 - Rising
    play_special(400, 300, false, true);
    play_special(430, 300, false, true);
    play_special(450, 300, false, true);
    play_special(500, 250, false, true);
    play_special(550, 250, false, true);

    // Set 2 - Beeping 1
    play_special(450, 100, false, true);
    play_special(400, 150, false, true);
    play_special(500, 250, false, true);
    play_special(550, 250, false, true);

    // Set 4 - Uh oh
    play_special(600, 900, true, true);
    play_special(500, 800, false, true);
    play_special(600, 900, true, true);

    // Set 5 - Fade out
    play_special(550, 100, false, true);
    play_special(540, 100, false, true);
    play_special(530, 100, false, true);
    play_special(520, 100, false, true);
    play_special(510, 100, false, true);
    play_special(500, 100, false, true);
    play_special(490, 100, false, true);
    play_special(480, 100, false, true);
    play_special(470, 100, false, true);
    play_special(460, 100, false, true);
    play_special(450, 1350, false, true);
}
