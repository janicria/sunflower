# Sunflower changelog

## Development 17
##### PANIC! macro 17/1/26

- Added the `PANIC!` macro & redid kernel panics
- Moved most exception handlers to `PANIC!`
- Cleaned up screenshots

## Development 16
##### Panic stack traces 14/1/26

- Gave kernel panics stack traces

## Development 15
##### Better panics 2/1/26

- Axed rbod and moved most of its code to the new `panic` module
- Touched up the `README` & some screenshots
- Finally removed `wait_no_ints`
- Improved the error message displayed by some floppy errors
- Touched up the `VGAChar` type

## Development 14
##### floppy & libfs de-spaghettification 30/12/25

- Overhauled libfs to make it actually usable now
- Moved the `fs::floppyfs` module in the kernel to `floppy`
- Moved the floppy reset functions to a new reset module
- Reorganized a lot of the floppy module

## Development 13
##### libfs 12/12/25

- Finally hlted during the keyboard poll loop, saving a LOT of wasted cpu resources
- Moved most of `fs.rs` & `floppyfs.rs` into `libfs` & updated `seeder` to check & test it
- Cropped a screenshot which needed cropping
- Made the year field in `VERSION` the current year because that just makes more sense
- Added a new kernel test for continuing after breakpoints and invalid opcodes

## Development 12
##### Misc improvements 8/12/25

- Added the new `ExitCode` return type to `startup::run`
- Updated screenshots!!
- Changed topbar color to grey
- Seeder actually compiles now
- Seeder now has better warnings when running with strange audio flag combinations
- Add floppy disk statistics & make some floppy statics const
- Formatted kernel manifest

## Development 11
##### Version 3/12/25

- Moved the sysinfo version consts and floppyfs release into the `VERSION` file
- Allowed `Time` to know the current century
- Fixed the top corners in rbod being in the wrong spots
- Fixed up some small mistakes in READMEs, CHANGELOG & package versions

## Development 10
##### Seeder 3/12/25

- Added sunflower's very own build tool `seeder`, with it's own directory
- Prevented formatting the floppy drive from permanently locking up the floppy driver (whoops!)
- Fixed `libutil::AsBytes` from being very very unsafe to normal levels of unsafe
- Updated the nightly channel to `2025-11-22`
- Cleaned up some IDT handler code
- Fixed a typo in SysCmd 7

## Development 09
##### libutil 25/11/25

- Moved `src/wrappers.rs` into it's own `libutil` library
- Moved the main sunflower kernel code into the `kernel` directory
- Both these changes were in prep for the sunflower build tool I'm working, called `seeder`

## Development 08
##### FloppyFS 22/11/25

- Allowed the creation of inodes in the new `FloppyFS` filesystem
- Created a (somewhat terrible) filesystem abstraction layer via the `fs` module 
- Allowed writing to floppies
- Fixed printing at the rightmost column of the vga buffer causing chars to be printed a row below
- Fixed some rbod keyboard input checks

## Development 07
##### Floppies!! 11/10/25

- Allowed reading from floppies via the `floppy` module.
- Fully redid the `pic` module (it was really old and terrible)
- Forced `writeb` & `readb` to always use dummy waits
- Improved the documentation for `ports::Port`
- Fixed `interrupts::triple_fault` so that it actually triple faults
- Decreased the emergency stack's size

## Development 06
##### Tests & better vga 29/9/25

- Added a bunch of tests which can be ran through `cargo did-i-break-anything`
- Spilt the vga module into 3 smaller modules (`buffers`, `cursor` & `print`) 
- Improved SysCmd 1 (again)
- Updated  the SysInfo & Help screenshots

## Development 05
##### TSS, debug macros & better double faults 27/9/25

- Added a much needed TSS to prevent stack overflows from triple faulting
- Made an actually responsible double fault handler that doesn't just call rbod
- Added the `cargo run_debug` command for easier debugging (and also `dbg_info!` and `warn!` macros)
- Fixed some typos in SysCmd 7
- Improved the system info section in rbod and SysCmd 1
- Renamed `LoadDescriptorError`to `LoadRegisterError` since the TSS now uses it
- Prevented `vga::swap_buffers` from accidentally still using the stack, causing stack overflows when called
- Enabled the very important `yeet_expr` feature 
- Fixed some rare cases where the Topbar would be hidden
- Updated rust version to accommodate for `target-pointer-width` becoming an integer in `bootloader`

## Development 04
##### GDT & Topbar 20/9/25

- Sunflower now loads it's own GDT!!
- Added the topbar & help syscmd (SysCmd 7)
- Added the `LoadDescriptorError` wrapper for easier IDT & GDT errors
- Made the rbod border prettier
- Updated screenshots
- Gave `print!` and `println!` color options, to replace `print_color`
- Moved the version from the manifest file to sysinfo

## Development 03
##### Wrappers, better RTC & SysCmd 6 19/9/25

- Added the `InitLater` & `UnsafeFlag` wrapper types
- Replaced some static muts with safe counterparts
- Added RTC time sync error codes via new startup task
- Added a new system command - SysCmd 6
- Created some VGA cursor helper functions for some reason
- Moved `SYS_INIT` into many smaller `X_INIT` statics and added them to SysCmd 1
- Added clippy command which avoids compile error (cargo paperclip)
- Patched a bug where holding shift or SysRq when launching sunflower in a VM would causes the key to become stuck in the opposite state

## Development 02
##### Syscmds, sysinfo & RTC 13/9/25

- Added system commands, see `System Commands` section in README
- Added (probably) accurate launch time using the RTC 
- Moved rbod cpuid check to sysinfo
- Added some screenshots
- Added back disable_enter after accidentally removing it
- Cleaned up some unsafe code

## Development 01
##### Startup tasks & better keyboard 13/9/25

- Added startup tasks
- Redid PS/2 keyboard driver
- Improved timer handler
- Moved dummy int handlers to only be for IRQ 7 & 15
- Improved some comments & fixed typo in README
- Removed test error comments in kmain & intrinsics feature

## Development 00 
##### 8/9/25

- Added versioning and pretty much redid the entire kernel
- There's waaaay too many changes to write down here so I just won't
