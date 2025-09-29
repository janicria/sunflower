use core::{
    any::type_name,
    cell::SyncUnsafeCell,
    fmt::Display,
    marker::PhantomData,
    mem::MaybeUninit,
    ptr,
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};

/// A wrapper type to construct uninitialised instances of `T`, which can be safely given an initialised value later.
///
/// Designed to replace unnecessary `static mut`s.
pub struct InitLater<T> {
    cell: SyncUnsafeCell<MaybeUninit<T>>,
    /// 0 - Uninit,
    /// 1 - Initialising,
    /// 2 - Initialised
    state: AtomicU8,
}

/// The value isn't initialised. It can be written to but not read from.
const UNINIT: u8 = 0;

/// The value is part way through initialising. It can neither be written to nor read from.
const INITIALISING: u8 = 1;

/// The value is initialised. It can be read from to but not written to.
const INIT: u8 = 2;

impl<T> InitLater<T> {
    /// Creates a new uninitialised `InitLater`.
    pub const fn uninit() -> Self {
        InitLater {
            cell: SyncUnsafeCell::new(MaybeUninit::uninit()),
            state: AtomicU8::new(UNINIT),
        }
    }

    /// Tries to initialise the value.
    /// Returns the loaded `val` for your convenience
    pub fn init(&self, val: T) -> Result<&'static T, InitError<T>> {
        let state = self.state.load(Ordering::Relaxed);
        self.state.store(INITIALISING, Ordering::Relaxed);

        match state {
            UNINIT => {
                // Safety: The check above (hopefully) ensures there no other active references
                let val = unsafe { &mut *self.cell.get() }.write(val);
                self.state.store(INIT, Ordering::Relaxed);
                Ok(val)
            }
            state => Err(InitError::new(state)),
        }
    }

    /// Tries to read the contained value.
    pub fn read(&self) -> Result<&T, InitError<T>> {
        match self.state.load(Ordering::Relaxed) {
            // Safety: No mutations are able to happen if the value is initialised
            INIT => unsafe { Ok((*self.cell.get()).assume_init_ref()) },
            state => Err(InitError::new(state)),
        }
    }
}

/// The error returned from various `InitLater` functions.
#[derive(Debug)]
pub struct InitError<T> {
    state: u8,
    _marker: PhantomData<T>,
}

impl<T> InitError<T> {
    /// Creates a new error.
    fn new(state: u8) -> Self {
        InitError {
            state,
            _marker: PhantomData,
        }
    }
}

/// Allows being passed to startup::run
impl<T> Display for InitError<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // Remove the path to the type, only keeping its name.
        let name = type_name::<T>().rsplit("::").next().unwrap_or_default();

        let state = match self.state {
            UNINIT => "Uninit",
            INITIALISING => "Initialising",
            INIT => "Initialised",
            _ => "Unknown",
        };

        write!(f, "InitLater {name} was accessed while {state}!",)
    }
}

/// A wrapper type for to construct boolean flags which are `unsafe` write to, but safe to read from.
///
/// Designed to replace `AtomicBool` statics can cause UB when written to incorrectly.
pub struct UnsafeFlag {
    val: AtomicBool,
}

impl UnsafeFlag {
    /// Creates a new `UnsafeFlag`.
    pub const fn new(val: bool) -> Self {
        UnsafeFlag {
            val: AtomicBool::new(val),
        }
    }

    /// Returns whether the flag is set or not.
    pub fn load(&self) -> bool {
        self.val.load(Ordering::Relaxed)
    }

    /// Sets the flag to `val`.
    /// # Safety
    /// It's up to you why setting the value is unsafe.
    pub unsafe fn store(&self, val: bool) {
        self.val.store(val, Ordering::Relaxed);
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

// A wrapper type for easily creating descriptors for your descriptor tables.
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

    /// Returns an uninitialised descriptor.
    pub fn uninit() -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Tests that `InitLater` can only be initialised once.
    #[test_case]
    fn initlater_inits_once() {
        let init = InitLater::uninit();
        assert!(init.init(0x42).is_ok());
        assert!(init.init(0x43).is_err())
    }

    /// Tests that `InitLater` can't be read from before it's initialised.
    #[test_case]
    fn initlater_cant_read_before_init() {
        let init = InitLater::uninit();
        assert!(init.read().is_err());
        let val = init.init(0x42).unwrap();
        assert_eq!(val, &0x42)
    }
}
