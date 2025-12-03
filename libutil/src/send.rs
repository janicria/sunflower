use crate::InitError;
use core::{fmt::Display, ptr, slice};

/// A wrapper type for easily creating descriptors for your descriptor tables.
#[repr(C, packed)]
pub struct TableDescriptor<T> {
    size: u16,
    offset: *const T,
}

impl<T> TableDescriptor<T> {
    /// Creates a new descriptor pointing to `table`.
    pub fn new(table: &'static T) -> Self {
        TableDescriptor {
            size: (size_of::<T>() - 1) as u16,
            offset: table,
        }
    }

    /// Returns an invalid descriptor.
    pub fn invalid() -> Self {
        TableDescriptor {
            size: 0,
            offset: ptr::null(),
        }
    }
}

impl<T> PartialEq for TableDescriptor<T> {
    fn eq(&self, other: &Self) -> bool {
        self.size == other.size && self.offset as u64 == other.offset as u64
    }
}

impl<T> Display for TableDescriptor<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let size = self.size;
        write!(f, "size = {size} & offset = 0x{:x}", self.offset as u64)
    }
}

/// A wrapper type for easily checking if your register (`T`) loaded correctly.
pub enum LoadRegisterError<T> {
    Load(InitError<T>),
    Store(&'static str),
    Other(&'static str),
}

impl<T> From<InitError<T>> for LoadRegisterError<T> {
    fn from(err: InitError<T>) -> Self {
        LoadRegisterError::Load(err)
    }
}

impl<T> Display for LoadRegisterError<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LoadRegisterError::Load(e) => write!(f, "Failed load, {e}"),
            LoadRegisterError::Store(t) => write!(f, "Stored {t} doesn't match loaded {t}"),
            LoadRegisterError::Other(s) => write!(f, "{s}"),
        }
    }
}

/// Enables converting `self` into an array of bytes.
/// # Safety
/// All possible values of the given type must never contain any uninitialised bytes,
/// such as padding bytes and must not have any interior mutability.
pub unsafe trait AsBytes {
    /// Converts `self` into an array of bytes.
    fn as_bytes(&self) -> &[u8] {
        let ptr = self as *const _ as *const u8;
        // Safety: The data coming from self is non-null, aligned as well as forever valid due to the requirement of implementing AsBytes
        unsafe { slice::from_raw_parts(ptr, size_of_val(self)) }
    }
}

// Safety: If a value has no uninit bytes, then an array of it will also not have any.
unsafe impl<T> AsBytes for [T] where T: AsBytes {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that the `AsBytes` trait functions correctly.
    #[test]
    #[rustfmt::skip]
    fn as_bytes_works() {
        #[repr(C, packed)] 
        struct MyStruct { x: u16, y: u8 }
        unsafe impl AsBytes for MyStruct {}
        
        let bytes = MyStruct { x: 257, y: 18 }.as_bytes();
        assert_eq!(bytes[0], 1);
        assert_eq!(bytes[1], 1);
        assert_eq!(bytes[2], 18)
    }
}
