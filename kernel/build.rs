use chrono::{Datelike, Local};
use serde::Deserialize;
use std::{fs, io};
use thiserror::Error;
use toml::de;

/// The path to the version file.
const VERSION: &str = "../VERSION";

/// The parsed VERSION file.
#[derive(Deserialize)]
pub struct Version {
    kernel: Kernel,
    floppyfs: FloppyFs,
}

/// The kernel's version fields.
#[derive(Deserialize)]
pub struct Kernel {
    version_long: String,
    version_short: String,
    patch_quote: String,
}

/// When the filesystem driver was last updated.
#[derive(Deserialize)]
pub struct FloppyFs {
    day: u16,
    year: u16,
}

/// Parses the VERSION file ands sends it to sunflower through environment variables.
#[rustfmt::skip]
fn main() -> Result<(), ParseVersionError> {
    let buf = fs::read(VERSION)?;
    let version: Version = toml::from_slice(&buf)?;
    let century = Local::now().year() / 100;
    
    println!("cargo::rerun-if-changed={VERSION}");
    println!("cargo::rustc-env=SFK_VERSION_LONG={}", version.kernel.version_long);
    println!("cargo::rustc-env=SFK_VERSION_SHORT={}", version.kernel.version_short);
    println!("cargo::rustc-env=SFK_PATCH_QUOTE={}", version.kernel.patch_quote);
    println!("cargo::rustc-env=SFK_FLOPPYFS_YEAR={}", version.floppyfs.year);
    println!("cargo::rustc-env=SFK_FLOPPYFS_DAY={}", version.floppyfs.day);
    println!("cargo::rustc-env=SFK_TIME_CENTURY={}", century);

    Ok(())
}

#[derive(Error, Debug)]
enum ParseVersionError {
    #[error("failed reading the VERSION file: {0}")]
    IOError(#[from] io::Error),

    #[error("failed parsing the VERSION file: {0}")]
    ParseError(#[from] de::Error),
}
