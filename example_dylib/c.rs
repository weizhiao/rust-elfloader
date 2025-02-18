#![crate_type = "cdylib"]
#![no_std]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

extern "Rust" {
    fn print(s: &str);
    fn b() -> i32;
}

#[no_mangle]
fn c() -> i32 {
    unsafe {
        print("call c()");
        b() + 1
    }
}
