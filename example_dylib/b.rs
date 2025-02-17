#![crate_type = "cdylib"]
#![no_std]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

extern "C" {
    fn a() -> i32;
}

#[no_mangle]
fn b() -> i32 {
    unsafe { a() + 1 }
}
