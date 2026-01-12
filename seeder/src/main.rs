/* ---------------------------------------------------------------------------
    seeder - Sunflower's build tool, sunflowerkernel.org
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
    seeder/src/main.rs

    Seeder's entry point
*/

#![feature(exit_status_error)]

use clap::{
    Arg, ArgMatches, Command, arg,
    builder::{
        Styles,
        styling::{Color, RgbColor, Style},
    },
    command,
};
use cmd::{BUILT_KERNEL_IMG, RunCommand};
use std::process::{self, Command as Cmd};

/// The color used for headers and usage.
const CORNFLOWER_BLUE: Color = Color::Rgb(RgbColor(120, 172, 255));

/// The color used for literals.
const PURPLE_BLUE: Color = Color::Rgb(RgbColor(163, 158, 255));

mod cmd;

fn main() {
    let mut command = command!()
        .about("Sunflower's build tool, seeder")
        .override_usage("cargo sdr COMMAND [OPTIONS]")
        .styles(
            Styles::styled()
                .usage(Style::new().bold().fg_color(Some(CORNFLOWER_BLUE)))
                .header(Style::new().bold().fg_color(Some(CORNFLOWER_BLUE)))
                .literal(Style::new().bold().fg_color(Some(PURPLE_BLUE))),
        )
        .subcommand(
            Command::new("build, b")
                .alias("build")
                .alias("b")
                .about("Builds the kernel")
                .args(args()),
        )
        .subcommand(
            Command::new("run, r")
                .alias("run")
                .alias("r")
                .about("Builds then runs the kernel in QEMU, requires passing in an audio flag")
                .args(args()),
        )
        .subcommand(
            Command::new("did-i-break-anything, diba")
                .alias("did-i-break-anything")
                .alias("diba")
                .about("Runs tests on the kernel in QEMU")
                .args(args()),
        )
        .subcommand(
            Command::new("clippy, c")
                .alias("clippy")
                .alias("c")
                .about("Checks sunflower using clippy")
                .args(args()),
        )
        .subcommand(
            Command::new("dbg, d")
                .alias("dbg")
                .alias("d")
                .about("alias: run -dn")
                .args(args()),
        )
        .args(args());

    match command.clone().get_matches().subcommand() {
        None => _ = command.print_help(), // show help if no or unknown commands are specified
        Some(cmd) => {
            // Ok! we've gotten a command
            match cmd.0 {
                "build, b" => build(cmd.1),
                "run, r" => run(cmd.1),
                "did-i-break-anything, diba" => run_alldirs(&RunCommand::Test, cmd.1),
                "clippy, c" => run_alldirs(&RunCommand::Clippy, cmd.1),
                "dbg, d" => run(&Command::new("")
                    .args(args())
                    .get_matches_from(["", "-d", "-n"])),
                s => panic!("got unknown command: {s}"),
            }
        }
    }
}

/// Ran when the build command is specified.
fn build(args: &ArgMatches) {
    warn_unneeded_arg("build", "pipewire", args);
    warn_unneeded_arg("build", "pulseaudio", args);
    warn_unneeded_arg("build", "nosound", args);

    cmd::run_command(&RunCommand::Build, "./kernel", args);
}

/// Ran when the run command is specified.
fn run(args: &ArgMatches) {
    let pipe = args.get_flag("pipewire");
    let pulse = args.get_flag("pulseaudio");
    let nosound = args.get_flag("nosound");

    // Prevent using multiple audio options at once
    if (pipe & pulse) | (pipe & nosound) | (pulse & nosound) {
        println!(
            "error: options `--pipewire`, `--pulseaudio` and `--nosound` cannot be used together in any combination"
        );
        process::exit(4)
    }

    let audio = if pipe {
        "pipewire"
    } else if pulse {
        "pa"
    } else {
        if !nosound {
            println!("warning: no audio flag specified, assuming --nosound")
        }
        "none"
    };

    let monitor = if args.get_flag("debug") {
        &["-monitor", "stdio"]
    } else {
        &[] as &[&str]
    };

    cmd::run_command(&RunCommand::Build, "./kernel", args);
    println!("Running QEMU with audio driver `{audio}`...");

    // Run QEMU!!
    if let Err(e) = Cmd::new("qemu-system-x86_64")
        .args([
            "-drive",
            format!("format=raw,file={BUILT_KERNEL_IMG}").as_str(),
            "-drive",
            "format=raw,file=./floppy.img,if=floppy",
            "-audio",
            format!("driver={audio},model=virtio,id=speaker").as_str(),
            "--machine",
            "pcspk-audiodev=speaker",
        ])
        .args(monitor)
        .status()
    {
        println!(
            "error: failed running QEMU (qemu-system-x86_64): {e}\nDid you install QEMU from https://www.qemu.org/download/ ?"
        );
        process::exit(5)
    }
}

/// Runs command `cmd` in `kernel/`, `libutil/` and `seeder`, warning on any any arguments.
fn run_alldirs(cmd: &RunCommand, args: &ArgMatches) {
    /// The dirs which `cmd` will be ran in
    static DIRS: [&str; 4] = ["seeder", "libutil", "libfs", "kernel"];

    let str = cmd.as_str();
    warn_unneeded_arg(str, "debug", args);
    warn_unneeded_arg(str, "noenter", args);
    warn_unneeded_arg(str, "pipewire", args);
    warn_unneeded_arg(str, "pulseaudio", args);
    warn_unneeded_arg(str, "nosound", args);
    cmd::run_command(&RunCommand::Build, "./kernel", args);

    for dir in DIRS {
        println!("Running {str} in {dir}...");
        cmd::run_command(cmd, dir, args);
    }
}

/// Warns the user that they didn't need an argument.
fn warn_unneeded_arg(cmd: &str, arg: &str, args: &ArgMatches) {
    if args.get_flag(arg) {
        println!("warn: argument `--{arg}` is ignored when using command `{cmd}`")
    }
}

/// The optional arguments for seeder.
fn args() -> [Arg; 6] {
    [
        arg!(debug: -d --debug "Enables runtime debug tools and information"),
        arg!(noenter: -e --noenter "Prevents sunflower from detecting if the enter key is pressed"),
        arg!(path: -p --path <FILE> "The file to write the built bootable disk image to"),
        arg!(pipewire: -w --pipewire "Run with pipewire audio support"),
        arg!(pulseaudio: -a --pulseaudio "Run with pulseaudio audio support"),
        arg!(nosound: -n --nosound "Run without audio"),
    ]
}
