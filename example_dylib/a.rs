#![crate_type = "cdylib"]
#![no_std]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
fn a() -> i32 {
    1
}

#[no_mangle]
pub static HELLO: &str = "Hello!";