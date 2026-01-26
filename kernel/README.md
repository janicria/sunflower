## Sunflower ~ kernel/

This directory contains the actual kernel binary used by sunflower and loaded by the bootloader. It's structure is as follows

```
sunflower/kernel/ 
   src/                   - Where the actual code goes 
   build.rs               - Build script to pass info to sunflower
   x86_64-sunflower.json  - The target that sunflower is built to
   README.md              - The file you're reading!
   everything else        - Config files used by cargo

```

