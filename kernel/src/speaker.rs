/* ---------------------------------------------------------------------------
    Sunflower kernel - sunflowerkernel.org
    Copyright (C) 2026 janicria

    This program is free software: you can redistribute it and/or modify
    it under the terms of the GNU General Public License as published by
    the Free Software Foundation, either version 3 of the License, or
    (at your option) any later version.

    This program is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
    GNU General Public License for more details.

    You should have received a copy of the GNU General Public License
    along with this program.  If not, see <https://www.gnu.org/licenses/>.
--------------------------------------------------------------------------- */

/*!
    kernel/src/speaker.rs

    Allows playing sounds through the PC speaker
*/

use crate::{
    ports::{self, Port},
    startup, time,
};

/// The bits required for the PC speaker to play sound through PIT channel 2.
static PLAY_BITS: u8 = 0b11;

/// Plays a sound with the specified frequency to the pc speaker.
///
/// Note: QEMU doesn't seem to be able to rapidly switch between playing different sounds
/// when used with certain headphones and requires passing
/// `-audio driver=<insert driver here>,model=virtio,id=speaker --machine pcspk-audiodev=speaker`
pub fn play(freq: u32) {
    // todo: explain why the command is 0b10110110
    static COMMAND: u8 = 0b10110110;

    if !startup::PIT_INIT.load() {
        warn!("attempted playing sounds with an uninit PIT!");
        return;
    }

    unsafe {
        // Set the second channel in the PIT to freq
        let freq = time::PIT_BASE_FREQ as u32 / freq;
        ports::writeb(Port::PITCmd, COMMAND);
        ports::writeb(Port::PITChannel2, freq as u8); // low byte
        ports::writeb(Port::PITChannel2, (freq >> 8) as u8); // high byte

        // If the play bits are not set, enable them
        let val = ports::readb(Port::PCSpeaker);
        if val != val | PLAY_BITS {
            ports::writeb(Port::PCSpeaker, val | PLAY_BITS);
        }
    }
}

/// Stops the current sound the pc speaker is playing.
pub fn stop() {
    // Safety: We're just disabling the play bits
    unsafe {
        // Disable the play bits
        let val = ports::readb(Port::PCSpeaker) & !PLAY_BITS;
        ports::writeb(Port::PCSpeaker, val);
    }
}

/// Plays `freq` for `time` milliseconds.
///
/// Repeatedly plays then stops playing at 100ms intervals if `repeat` is set.
pub fn play_special(freq: u32, millis: u64, repeat: bool) {
    // FIXME: make this actually convert from milliseconds by dividing by 10 (yet still have 'songs' sound good)
    let ticks = millis / 13; // convert millis to ticks

    if !startup::PIT_INIT.load() {
        warn!("attempted playing special with an uninit PIT!");
        return;
    }

    if repeat {
        static PULSE_LENGTH: u64 = 6;
        for _ in 0..ticks / (PULSE_LENGTH * 2) {
            play(freq);
            time::wait(PULSE_LENGTH);
            stop();
            time::wait(PULSE_LENGTH);
        }
    } else {
        play(freq);
        time::wait(ticks);
        stop();
    }
}

/// Plays the boot chime.
pub fn play_chime() {
    play_special(600, 350, false);
    play_special(620, 450, false);

    play_special(600, 350, false);
    play_special(780, 900, false);
}

/// Plays when an rbod has occurred and everything is wrong in the world.
pub fn play_song() {
    // Set 1 - Rising
    play_special(400, 300, false);
    play_special(430, 300, false);
    play_special(450, 300, false);
    play_special(500, 250, false);
    play_special(550, 250, false);

    // Set 2 - Beeping 1
    play_special(450, 100, false);
    play_special(400, 150, false);
    play_special(500, 250, false);
    play_special(550, 250, false);

    // Set 3 - Beeping 2
    play_special(600, 100, false);
    play_special(620, 100, false);
    play_special(600, 100, false);
    play_special(620, 100, false);
    play_special(600, 100, false);
    play_special(620, 100, false);
    play_special(500, 100, false);
    play_special(480, 100, false);

    // Set 2 - Beeping 1
    play_special(450, 100, false);
    play_special(400, 150, false);
    play_special(500, 250, false);
    play_special(550, 250, false);

    // Set 1 - Rising
    play_special(400, 300, false);
    play_special(430, 300, false);
    play_special(450, 300, false);
    play_special(500, 250, false);
    play_special(550, 250, false);

    // Set 2 - Beeping 1
    play_special(450, 100, false);
    play_special(400, 150, false);
    play_special(500, 250, false);
    play_special(550, 250, false);

    // Set 4 - Uh oh
    play_special(600, 900, true);
    play_special(500, 800, false);
    play_special(600, 900, true);

    // Set 5 - Fade out
    play_special(550, 100, false);
    play_special(540, 100, false);
    play_special(530, 100, false);
    play_special(520, 100, false);
    play_special(510, 100, false);
    play_special(500, 100, false);
    play_special(490, 100, false);
    play_special(480, 100, false);
    play_special(470, 100, false);
    play_special(460, 100, false);
    play_special(450, 1350, false);
}
