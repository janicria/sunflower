use super::{
    BLOCK_START, DualBlockPtr, FileMode, FilesystemFeatures, FilesystemHeader, INODE_START, INODES,
    INode, MAGIC,
};
use crate::{
    floppy::{
        FloppyError,
        disk::{self, CYLINDERS, HEADS, SECTOR_SIZE, SECTORS},
    },
    fs::INODES_PER_BLOCK,
    interrupts, startup, time,
    wrappers::{AsBytes, ExclusiveMap},
};
use core::mem;
use thiserror::Error;

/// The last sector in the floppy drive
const END_SECTOR: u16 = CYLINDERS * HEADS * SECTORS;

/// Memory cached inode table, should be written to disk on write.
static INODE_TABLE: [ExclusiveMap<INode>; INODES] =
    [const { ExclusiveMap::new(INode::zeroed()) }; INODES];

/// Bitmap of freely available blocks, a zero indicates that a block is available.
static FREE_BLOCKS: [ExclusiveMap<u8>; INODES * 8] = [const { ExclusiveMap::new(0) }; INODES * 8];

/// A good default filesystem header.
const GOOD_FS_HEADER: FilesystemHeader = FilesystemHeader {
    magic: MAGIC,
    // 22nd November 2025 UTC
    release: 0 << 10 | 326,
    features: FilesystemFeatures::FLOPPY,
    name: [
        // "floppy drive"
        102, 108, 111, 112, 112, 121, 32, 100, 114, 105, 118, 101, 0, 0, 0, 0,
    ],
    // mount it as the root dir
    mountpoint: [0; 64],
    _reserved: [0; 416],
};

/// Reformats the floppy drive after prompting the user.
fn reformat_drive() -> Result<(), InitError> {
    if !interrupts::kbd_wait_for_response("Format floppy drive", true) {
        return Err(InitError::CorruptDrive);
    }
    println!("Formatting floppy drive...");

    // Write a new fs header
    let fsheader = GOOD_FS_HEADER.as_bytes();
    disk::write(0, &fsheader)?;

    // Zero out the inode table
    let inodes = [const { INode::zeroed() }; INODES].as_bytes();
    disk::write(INODE_START, &inodes)?;

    Ok(())
}

/// Initialises and mounts the floppy filesystem.
#[allow(unused_variables)]
pub fn init_floppyfs() -> Result<(), InitError> {
    if !startup::FLOPPY_INIT.load() {
        return Err(InitError::NoFloppyDriver);
    }

    // Read the filesystem's header
    let mut buf = [0; size_of::<FilesystemHeader>()];
    disk::read(0, &mut buf)?;
    let fsheader: FilesystemHeader = FilesystemHeader::from_raw(buf);

    if fsheader.magic != MAGIC {
        dbg_info!("Bad filesystem magic found");
        reformat_drive()?
    }

    // Check if the filesystem is a newer version
    let fs_year = fsheader.release >> 10;
    let cur_year = GOOD_FS_HEADER.release >> 10;
    if fs_year > cur_year || (fs_year == cur_year && fsheader.release > GOOD_FS_HEADER.release) {
        dbg_info!("Filesystem has newer release than kernel, some features may not be supported")
    }

    let feats = fsheader.features;
    dbg_info!(
        "Found floppy filesystem: {}, released {}:{}\nFilesystem features: {}",
        str::from_utf8(&fsheader.name).unwrap_or("filesystem contains bad name"),
        fsheader.release & 0b111111111, // show only day
        fs_year + 2025,                 // add start year
        feats.0
    );

    // Read the inode table
    let mut active = 0u32;
    let mut buf = [0; size_of::<INode>() * INODES];
    let cyl0 = size_of::<INode>() * 17 * INODES_PER_BLOCK;
    let cyl1 = cyl0 + size_of::<INode>() * 18 * INODES_PER_BLOCK;
    disk::read(INODE_START, &mut buf[..cyl0])?; // read first cyl
    disk::read(INODE_START + 17, &mut buf[cyl0..cyl1])?; // read second cyl
    // Safety: All bit patterns of inode are safe
    let nods = unsafe { mem::transmute::<[u8; size_of::<INode>() * INODES], [INode; INODES]>(buf) };

    // Update the memory-based table
    for (idx, exmap) in INODE_TABLE.iter().enumerate() {
        let inode = nods[idx].clone();
        let (mode, meta) = (inode.mode, inode.meta.clone());
        exmap.map(|v| *v = inode).ok_or(InitError::ExmapFailure)?;

        if mode.contains(FileMode::ACTIVE) {
            active += 1;

            // Update free block bitmap
            for ptrs in meta.iter() {
                for ptr in ptrs.decode().into_iter().filter(|p| *p != 0) {
                    alloc_bmp(ptr).ok_or(InitError::ExmapFailure)?;
                }
            }
        }
    }
    dbg_info!("Read inode table, active inodes: {active}");

    Ok(())
}

/// An error created when trying to initialise the floppy filesystem.
#[derive(Error, Debug)]
pub enum InitError {
    #[error("the floppy driver failed!")]
    NoFloppyDriver,

    #[error("floppy error: {0}")]
    FloppyError(#[from] FloppyError),

    #[error("the floppy drive was corrupt!")]
    CorruptDrive,

    // should be impossible
    #[error("unable to access an exclusive map value")]
    ExmapFailure,
}

/// Marks the `ptr`th block in the block bitmap as used, then returns it's previous value.
fn alloc_bmp(ptr: u16) -> Option<bool> {
    let addr = (ptr as usize) / 8;
    let bit = 7 - (ptr % 8);

    FREE_BLOCKS[addr].map(|i| {
        let prev = (*i >> bit) & 1 == 1; // is bit set?
        *i |= 1 << bit; // set bit
        prev
    })
}

/// Attempts to allocate the first available inode  in the table giving it `size`, `mode` and `blocks` blocks.
///
/// Returns a pointer to the newly allocated inode it on success.
pub fn alloc_inode(size: i16, mode: FileMode, blocks: u8) -> Result<u64, AllocINodeError> {
    /// Checking if inodes and blocks are available can spuriously fail due to the use of exmaps
    const RETRIES: u8 = 4;

    let blocks = blocks.min(32) as usize;
    let mut err = AllocINodeError::OutOfInodes;

    for _ in 0..RETRIES {
        let mut alloc_blks = 0; // the number of blocks we've allocated
        let mut use_ptr2 = false; // do we use the first or second ptr for the next block?
        let mut block_ptrs = [const { DualBlockPtr([0; 3]) }; 32];

        // Find those blocks!
        for ptr in BLOCK_START..END_SECTOR {
            if alloc_blks == blocks {
                break; // we've allocated the right amount of blocks!
            }

            if let Some(used) = alloc_bmp(ptr)
                && !used
            {
                // We found an available block!
                let mut ptrs = block_ptrs[alloc_blks].decode();
                ptrs[use_ptr2 as usize] = ptr;
                block_ptrs[alloc_blks] = DualBlockPtr::encode(ptrs);

                use_ptr2 = !use_ptr2;
                alloc_blks += 1;
            }
        }

        // Check if we found enough blocks to allocate
        if blocks != 0 && block_ptrs[blocks - 1].decode()[0] == 0 {
            err = AllocINodeError::OutOfBlocks;
            break;
        }

        // Try find an inode
        err = AllocINodeError::OutOfInodes;
        for (idx, exmap) in INODE_TABLE.iter().enumerate() {
            let mut inode_ptr = None;
            if let Some(()) = exmap.map(|nod| {
                if !nod.mode().contains(FileMode::ACTIVE) {
                    // We found an available inode!
                    nod.mode = mode | FileMode::ACTIVE;
                    nod.links = 1;
                    nod.size = size;
                    nod.meta = block_ptrs.clone(); // annoying clone because compiler sucks
                    inode_ptr = Some(idx as u64);
                }
            }) && let Some(ptr) = inode_ptr
            {
                write_inode(ptr)?;
                dbg_info!("created new floppyfs inode at idx {ptr}");
                return Ok(ptr);
            };
        }
    }

    Err(err)
}

/// The error returned when trying to initialise an inode.
#[derive(Error, Debug)]
pub enum AllocINodeError {
    #[error("ran out of available inodes, delete some files to regain entries")]
    OutOfInodes,

    #[error("ran out of blocks on the filesystem, looks like all storage has been used up")]
    OutOfBlocks,

    #[error(transparent)]
    WriteError(#[from] InodeIOError),
}

/// Writes the `ptr`th inode to disk. May block for up to 20 ms.
fn write_inode(ptr: u64) -> Result<(), InodeIOError> {
    let mut buf = [const { INode::zeroed() }; 4];
    let lba = (ptr as usize) & !0b11; // lowest multiple of 4

    if lba > INODES {
        return Err(InodeIOError::NoInodeFound(ptr));
    }

    // Try to read from the table
    let start = time::get_time();
    'timer: while time::timer(start, 1) {
        for (idx, inode) in INODE_TABLE[lba..lba + 4].iter().enumerate() {
            if inode.map(|nod| buf[idx] = nod.clone()).is_none() {
                continue 'timer;
            }
        }

        return disk::write(INODE_START + lba as u16, &buf.as_bytes()).map_err(Into::into);
    }

    Err(InodeIOError::TableBusy)
}

/// Reads the data in `ptr`th inode from disk `buf` and returns the number of sectors read.
/// May block for up to 20 ms.
pub fn read_inode(ptr: u64, buf: &mut [u8]) -> Result<u16, InodeIOError> {
    let exmap = INODE_TABLE
        .get(ptr as usize)
        .ok_or(InodeIOError::NoInodeFound(ptr))?;

    // Try to read from the table
    let start = time::get_time();
    let mut ptrs = [const { DualBlockPtr([0; 3]) }; 32];
    while time::timer(start, 1) {
        if exmap.map(|nod| ptrs = nod.meta.clone()).is_none() {
            continue;
        }

        // Ok! ptrs now contains the blocks we need to read from
        let mut read = 0;
        let mut tmp_buf = [0; SECTOR_SIZE];
        for ptrs in ptrs.iter().map(|p| p.decode()) {
            print!("| ptrs: {:?}", &ptrs);
            for ptr in ptrs.into_iter().filter(|ptr| *ptr != 0) {
                if buf.len() < (read + 1) * SECTOR_SIZE {
                    // we've hit the end of the buffer
                    return Ok(read as u16);
                }

                disk::read(ptr, &mut tmp_buf)?;
                buf[read * SECTOR_SIZE..(read + 1) * SECTOR_SIZE].copy_from_slice(&tmp_buf);
                read += 1;
            }
        }

        return Ok(read as u16);
    }

    Err(InodeIOError::TableBusy)
}

/// The error returned when trying to write and write inodes to/from disk.
#[derive(Error, Debug)]
pub enum InodeIOError {
    #[error("the inode task couldn't be accessed in a reasonable amount of time")]
    TableBusy,

    #[error("floppy driver error: {0}")]
    FloppyError(#[from] FloppyError),

    #[error("no inode found with index {0}")]
    NoInodeFound(u64),
}
