use crate::InitError;
use core::{fmt::Display, mem, ptr};

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
pub trait AsBytes {
    /// Converts `self` into an array of bytes.
    fn as_bytes(&self) -> [u8; size_of::<Self>()]
    where
        Self: Sized;
}

impl<T> AsBytes for T {
    fn as_bytes(&self) -> [u8; size_of::<Self>()]
    where
        Self: Sized,
    {
        // Safety: A [u8; size_of::<Self>()] always has the same size as Self, and is always valid.
        unsafe { mem::transmute_copy(self) }
    }
}

#[cfg(test)]
mod tests {
    use crate::AsBytes;

    /// Tests that the `AsBytes` trait functions correctly.
    #[test]
    #[rustfmt::skip]
    fn as_bytes_works() {
        #[repr(C, packed)] 
        struct MyStruct { x: u16, y: u8 }
        let bytes = MyStruct { x: 257, y: 18 }.as_bytes();
        assert_eq!(bytes[0], 1);
        assert_eq!(bytes[1], 1);
        assert_eq!(bytes[2], 18)
    }
}
