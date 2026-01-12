/* ---------------------------------------------------------------------------
    libutil - Sunflower kernel utility library, sunflowerkernel.org
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
    libutil/src/lib.rs

    Library root file
*/

#![cfg_attr(not(test), no_std)]
#![feature(sync_unsafe_cell)]

pub use send::{AsBytes, LoadRegisterError, TableDescriptor};
pub use sync::{ExclusiveMap, InitError, InitLater, UnsafeFlag};

pub mod sync;
pub mod send;
