# Sunflower changelog

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
