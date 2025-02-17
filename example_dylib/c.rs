#![crate_type = "cdylib"]
#![no_std]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

extern "Rust" {
    fn a() -> i32;
    fn b() -> i32;
}

#[no_mangle]
fn c() -> i32 {
    unsafe { a() + b() }
}
