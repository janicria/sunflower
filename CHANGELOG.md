# Sunflower changelog

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
