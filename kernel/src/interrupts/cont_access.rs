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

#![allow(unused)]

#[cfg(test)]
use core::sync::atomic::{AtomicU8, Ordering};
use core::{cell::SyncUnsafeCell, fmt::Debug};
#[cfg(not(test))]
use {crate::PANIC, core::any::type_name};

/// Count of how far down in interrupt handlers we are,
/// where a zero means that we're not in an interrupt handler.
#[unsafe(export_name = "int_handler_count")]
static mut INT_HANDLER_COUNT: u32 = 0;

/// Incremented whenever `get_ref` or `write` would invoke a `badbug`.
#[cfg(test)]
static CONT_ACCESS_PANICS: AtomicU8 = AtomicU8::new(0);

/// Continuous access type.
///
/// Allows obtain references of and writing to [`Sync`] types without ANY
/// locking, blocking or failing, so long as the [`ContAccess`] is **NEVER**
/// accessed by any interrupt handlers (it's only accessed continuously).
///
/// This allows [`ContAccess`]es to be mutated while immutable references to
/// them are active, as can be seen in the below code example:
/// ```
/// let cont: ContAccess<u32> = ContAccess::new(0x42);
/// let cont_ref: &u32 = cont.get_ref();
/// cont.write(87); // allowed!
/// print!("{}", cont_ref); // prints '87'
/// ```
struct ContAccess<T: Sync>(SyncUnsafeCell<T>);

impl<T: Sync + Debug> ContAccess<T> {
    /// Creates a new cont access.
    pub fn new(v: T) -> ContAccess<T> {
        ContAccess(SyncUnsafeCell::new(v))
    }

    /// Returns a reference to the contained value.
    ///
    /// Panics if in an interrupt handler.
    pub fn get_ref(&self) -> &T {
        // Safety: Only write is some incs in int handlers
        let cnt = unsafe { INT_HANDLER_COUNT };
        if cnt != 0 {
            #[cfg(test)]
            CONT_ACCESS_PANICS.fetch_add(1, Ordering::Relaxed);
            #[cfg(not(test))]
            PANIC!(badbug "hit ContAccess:get_ref with a handler count of {cnt}\nAccess type: {}", type_name::<T>())
        }
        unsafe { &*self.0.get() }
    }

    /// Writes `v` into the contained value.
    ///
    /// Panics if in an interrupt handler.
    pub fn write(&self, v: T) {
        // Safety: Only write is some incs in int handlers
        let cnt = unsafe { INT_HANDLER_COUNT };
        if cnt != 0 {
            #[cfg(test)]
            CONT_ACCESS_PANICS.fetch_add(1, Ordering::Relaxed);
            #[cfg(not(test))]
            PANIC!(badbug "hit ContAccess:write with a handler count of {cnt}\nWrite value: {v:?} Access type: {}", type_name::<T>())
        }
        unsafe { *self.0.get() = v }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that [`ContAccess::get_ref`] and [`ContAccess::write`] panic when [`INT_HANDLER_COUNT`] is nonzero.
    #[test_case]
    fn cont_access_panics() {
        let cont = ContAccess::new(0x42);
        let assert_fails = |fails: u8| {
            let _ = cont.get_ref();
            cont.write(0x42);
            assert_eq!(CONT_ACCESS_PANICS.load(Ordering::Relaxed), fails);
        };

        assert_fails(0);
        unsafe { INT_HANDLER_COUNT += 1 };
        assert_fails(2);
        unsafe { INT_HANDLER_COUNT -= 1 };
        assert_fails(2);
    }

    /// Tests that references obtained from [`ContAccess::get_ref`] update after [`ContAccess::write`]s.
    #[test_case]
    fn refs_update() {
        let cont = ContAccess::new(0x42);
        let cont_ref = cont.get_ref();
        assert_eq!(*cont_ref, 0x42);
        cont.write(87);
        assert_eq!(*cont_ref, 87);
        assert_eq!(cont_ref, cont.get_ref())
    }
}
