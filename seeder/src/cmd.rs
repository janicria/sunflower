use clap::ArgMatches;
use std::{
    fs::{self, OpenOptions},
    io,
    process::{self, Command, ExitStatusError},
};
use thiserror::Error;

/// The path of the built kernel image.
pub const BUILT_KERNEL_IMG: &str = "kernel/target/x86_64-sunflower/release/bootimage-sunflower.bin";

/// The path of the copied kernel image.
const COPIED_KERNEL_IMG: &str = "sunflower.bin";

/// A command which can be ran by seeder.
#[derive(PartialEq)]
pub enum RunCommand {
    Build,
    Clippy,
    Test,
}

impl RunCommand {
    /// Converts the command to a `&str`.
    pub fn as_str(&self) -> &str {
        match self {
            RunCommand::Build => "bootimage",
            RunCommand::Clippy => "paperclip",
            RunCommand::Test => "did-i-break-anything",
        }
    }
}

/// Runs command `cmd` in dir `dir`, installing bootimage if required and aborting if any errors occured.
/// See `kernel/.cargo/config.toml` for a list of commands.
pub fn run_command(cmd: &RunCommand, dir: &str, args: &ArgMatches) {
    let cmd_str = cmd.as_str();
    if let Err(e) = try_run(cmd_str, dir, args) {
        if *cmd != RunCommand::Build {
            println!("error: failed running command {cmd_str}: {e}");
            process::exit(6)
        }

        // cargo couldn't run bootimage... :c
        println!("Installing bootimage build tool...");

        if let Err(e) = Command::new("cargo")
            .args(["install", "bootimage@0.10.3"])
            .status()
        {
            // cargo couldn't install bootimage?
            println!("error: running `cargo` to install bootimage@0.10.3, {e}");
            process::exit(1)
        }

        // ok! we installed bootimage
        if let Err(e) = try_run(cmd_str, dir, args) {
            println!("error: failed running build command, {e}");
            process::exit(2)
        }
    }

    // Create floppy drive if it didn't already exist
    if OpenOptions::new().read(true).open("floppy.img").is_err() {
        // no floppy drive!
        println!("Creating floppy drive...");
        if let Err(e) = fs::write("floppy.img", [0u8; 1440 * 1024]) {
            println!("error: failed created floppy.img, {e}");
            process::exit(3)
        }
    }

    // just need to copy over the bin and we're done!
    if *cmd == RunCommand::Build {
        let path = if let Some(path) = args.get_one("path") {
            path
        } else {
            &String::from(COPIED_KERNEL_IMG)
        };
        println!("Built kernel image at `{BUILT_KERNEL_IMG}`, copying to `{path}`...");
        if fs::copy(BUILT_KERNEL_IMG, path).is_err() {
            println!(
                "warn: failed copying kernel image, yet a built image of sunflower still exists at `{path}`"
            );
        } else {
            println!("Successfully built bootable sunflower image located at `{path}`")
        }
    }
}

/// Attempts to run command `cmd`, returning false if any errors occured.
fn try_run(cmd: &str, dir: &str, args: &ArgMatches) -> Result<(), RunCargoError> {
    // Check for any features
    let debug = args.get_flag("debug");
    let noenter = args.get_flag("noenter");
    let feats: &[&str] = if debug && noenter {
        &["-F", "debug_info", "disable_enter"]
    } else if debug {
        &["-F", "debug_info"]
    } else if noenter {
        &["-F", "disable_enter"]
    } else {
        &[]
    };

    let path = fs::canonicalize(dir).expect("no kernel/ directory found!");
    let mut build_cmd = Command::new("cargo");
    let build_cmd = build_cmd
        .args([cmd, "--manifest-path", "./Cargo.toml"])
        .args(feats)
        .current_dir(path);
    build_cmd.status()?.exit_ok().map_err(Into::into)
}

#[derive(Error, Debug)]
enum RunCargoError {
    #[error("failed running cargo, {0}")]
    CouldntRun(#[from] io::Error),

    #[error("cargo returned error: {0}")]
    BadExitStatus(#[from] ExitStatusError),
}
