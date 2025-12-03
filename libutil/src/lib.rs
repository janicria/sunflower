//! Library for some useful utilities used by sunflower and it's libraries.

#![cfg_attr(not(test), no_std)]
#![feature(sync_unsafe_cell)]

pub use send::{AsBytes, LoadRegisterError, TableDescriptor};
pub use sync::{ExclusiveMap, InitError, InitLater, UnsafeFlag};

/// Useful synchronization types.
pub mod sync;

/// Useful types when you need to send data in weird ways.
pub mod send;
