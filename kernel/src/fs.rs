// TO BE REMOVED ON NEXT PATCH

pub use floppyfs::{FLOPPYFS_INIT, alloc_inode, init_floppyfs, read_inode};

/// A floppy disk connected filesystem.
mod floppyfs;