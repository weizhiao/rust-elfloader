#![crate_type = "cdylib"]
#![no_std]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

extern "Rust" {
    fn print(s: &str);
}

#[no_mangle]
fn a() -> i32 {
    unsafe {
        print("call a()");
        1
    }
}
