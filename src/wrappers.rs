use core::{
    any::type_name,
    cell::SyncUnsafeCell,
    error::Error,
    fmt::{Debug, Display},
    marker::PhantomData,
    mem::{self, MaybeUninit},
    ptr,
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};

/// A wrapper type to construct uninitialised instances of `T`, which can be safely given an initialised value later.
///
/// Designed to replace unnecessary `static mut`s.
#[derive(Debug)]
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
    pub fn init(&self, val: T) -> Result<&T, InitError<T>> {
        let state = self.state.load(Ordering::Relaxed);
        self.state.store(INITIALISING, Ordering::Relaxed);

        match state {
            UNINIT => {
                // Safety: The check above (hopefully) ensures there no other active references
                let val = unsafe { &mut *self.cell.get() }.write(val);
                self.state.store(INIT, Ordering::Relaxed);
                Ok(val)
            }
            state => {
                self.state.store(state, Ordering::Relaxed);
                Err(InitError::new(state))
            }
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
#[derive(Debug, PartialEq)]
pub struct InitError<T> {
    pub state: u8,
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

impl<T: Debug> Error for InitError<T> {}

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

/// A mutually exclusive piece of data only accessible by applying a function on it.
pub struct ExclusiveMap<T> {
    cell: SyncUnsafeCell<T>,
    access: AtomicBool,
}

impl<T> ExclusiveMap<T> {
    /// Creates a new map using `val` as it's contained value.
    pub const fn new(val: T) -> Self {
        ExclusiveMap {
            cell: SyncUnsafeCell::new(val),
            access: AtomicBool::new(false),
        }
    }

    /// Applies `f` to the contained value then returns what it returned.
    ///
    /// Fails and returns `None` if another instance of `map` is in progress.
    pub fn map<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut T) -> R,
    {
        if self
            .access
            // as far as I'm aware, the cmpxchg and cmpxchg weak intrinsics translate to the same set of instructions on x86
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            // Safety: The check above ensures that we have exclusive access to cell
            let val = unsafe { &mut *self.cell.get() };
            let ret = f(val);
            self.access.store(false, Ordering::Relaxed);
            Some(ret)
        } else {
            None
        }
    }
}

/// A wrapper type for to construct boolean flags which are `unsafe` write to, but safe to read from.
///
/// Designed to replace `AtomicBool` statics can cause UB when written to incorrectly.
#[derive(Debug)]
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
        // Safety: A [u8; size_of::<Self>()] always has the same size as Self, as is always valid.
        unsafe { mem::transmute_copy(self) }
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

    /// Tests that `ExclusiveMap` can be written to and read from correctly.
    #[test_case]
    fn exclusive_map_works() {
        let exmap = ExclusiveMap::new(42);
        exmap.map(|i| *i += 8).unwrap();
        exmap.map(|i| assert_eq!(*i, 50)).unwrap();
        exmap.map(|_| assert!(exmap.map(|_| {}).is_none())).unwrap()
    }
}
