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
    kernel/src/interrupts/cont_access.rs

    Defines the [`ContAccess`] type.
    Contained within the interrupts module
*/

#![allow(dead_code)]

use core::cell::SyncUnsafeCell;
#[cfg(test)]
use core::sync::atomic::AtomicU8;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
#[cfg(not(test))]
use {crate::PANIC, core::any::type_name};

/// Count of how far down in interrupt handlers we are,
/// where a zero means that we're not in an interrupt handler.
#[unsafe(export_name = "int_handler_count")]
static INTERRUPT_DEPTH: AtomicU32 = AtomicU32::new(0);

/// Incremented whenever [`ContAccess::check_access`] would fail.
#[cfg(test)]
static CONT_ACCESS_PANICS: AtomicU8 = AtomicU8::new(0);

/// The continuous access type, (or just CA for short).
///
/// Basically just `Cell` and `ExclusiveMap` combined, but without all of the
/// annoyances they give when you try to actually use them.
///
/// The only rules when accessing a CA is to **NOT**
///
/// - access a CA inside an interrupt handler,
/// - or inside a call to it's [`ContAccess::btemap`]
///
/// doing so will cause a `badbug` to be triggered and
/// the kernel to crash horrifically, ruining everyone's day.
/// ```
pub struct ContAccess<T> {
    data: SyncUnsafeCell<T>,
    /// Set when in btemap, fails check_access
    locked: AtomicBool,
}

impl<T> ContAccess<T> {
    /// Creates a new CA from the given `v`.
    pub const fn new(v: T) -> ContAccess<T> {
        ContAccess {
            data: SyncUnsafeCell::new(v),
            locked: AtomicBool::new(false),
        }
    }

    /// Checks that the CA isn't locked or in an interrupt handler,
    /// triggering a `badbug` if so.
    ///
    /// This means that if this function returns, it's guaranteed that this CA
    /// will never be accessed from anywhere else (due to CA's being amazing).
    ///
    /// Increments [`CONT_ACCESS_PANICS`] instead of triggering
    /// a `badbug` if in a test build.
    fn check_access(&self) {
        let locked = self.locked.load(Ordering::Relaxed);
        let int_depth = INTERRUPT_DEPTH.load(Ordering::Relaxed);

        // we want to print both locked & depth every fail
        if locked || int_depth != 0 {
            #[cfg(not(test))]
            PANIC!(badbug "ContAccess was accessed in a bad state
Interrupt depth: {int_depth} {}
Type: {}", if locked {"Locked"} else {""}, type_name::<T>());
            #[cfg(test)]
            CONT_ACCESS_PANICS.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Runs the passed function on the contained value, returning it's result.
    ///
    /// Accessing this CA in any other way inside the passed function triggers
    /// a `badbug`, as can be seen in the below example:
    ///
    /// ```
    /// let ca = ContAccess::new(42);
    /// ca.btemap(|v|{
    ///     println!("{v:?}"); // fine...
    ///     ca.write(15);      // BADBUG TRIGGERED!!
    ///     println!("{v:?}"); // unreachable
    /// })
    ///
    /// ```
    #[rustfmt::skip]
    pub fn btemap<R>(&self, f: impl FnOnce(&mut T) -> R) -> R { // BETTER THAN EXCLUSIVE MAP!!!
        self.check_access();
        // check_access ensures that locked is false, and the
        // nature of ContAccess ensures that it will stay false
        self.locked.store(true, Ordering::Relaxed);

        // SAFETY: check_access ensures that we have an exclusive access
        let res = unsafe { f(&mut *self.data.get()) };
        self.locked.store(false, Ordering::Relaxed);
        res
    }

    /// Sets the contained value to `val`.
    pub fn write(&self, val: T) {
        self.check_access();
        // SAFETY: check_access ensures that we have an exclusive access
        unsafe { *self.data.get() = val }
    }
}

impl<T: Copy> ContAccess<T> {
    /// Copies the contained value then returns it.
    pub fn copy(&self) -> T {
        self.check_access();
        // SAFETY: T: Copy allows copying out of self.data
        // and check_access ensures no active mutations
        unsafe { *self.data.get() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that [`ContAccess::check_access`] panics when
    /// [`INTERRUPT_DEPTH`] is nonzero.
    #[test_case]
    fn badbug_on_ints() {
        CONT_ACCESS_PANICS.store(0, Ordering::Relaxed);
        let ca = ContAccess::new(42);
        let assert_fails = |fails: u8| {
            ca.copy(); // test everything
            ca.write(15);
            ca.btemap(|_| {});
            assert_eq!(CONT_ACCESS_PANICS.load(Ordering::Relaxed), fails);
        };

        assert_fails(0);
        INTERRUPT_DEPTH.fetch_add(1, Ordering::Relaxed);
        assert_fails(3);
        INTERRUPT_DEPTH.fetch_sub(1, Ordering::Relaxed);
        assert_fails(3);
    }

    /// Tests that [`ContAccess::check_access`] panics when accessing it
    /// inside  of it's [`ContAccess::btemap`].
    #[test_case]
    fn badbug_on_locked() {
        CONT_ACCESS_PANICS.store(0, Ordering::Relaxed);
        let ca = ContAccess::new(42);
        ca.btemap(|_| ca.copy());
        ca.btemap(|v| ca.write(*v));
        assert_eq!(CONT_ACCESS_PANICS.load(Ordering::Relaxed), 2);
    }

    /// Tests that [`ContAccess::btemap`], [`ContAccess::copy`] and
    /// [`ContAccess::write`] all work correctly.
    #[test_case]
    fn everything_works_fine() {
        let ca = ContAccess::new(42);
        assert_eq!(ca.copy(), 42);
        ca.btemap(|ans| assert_eq!(*ans, 42));
        ca.write(87);
        assert_eq!(ca.copy(), 87);
        assert_eq!(ca.btemap(|ans| *ans), 87);
    }
}
