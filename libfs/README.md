## Sunflower ~ libfs/

This directory contains the `libfs` library used for creating, reading and modifying sunflower readable filesystems. It's structure is as follows

```
sunflower/libfs/ 
   src/lib.rs         # Library root file
   src/init.rs        # Utility functions used to help initialise a filesystem.
   src/table.rs       # Handles the inode table.
   Cargo.toml         # Config file used by cargo   
   README.md          # The file you're reading!
```
