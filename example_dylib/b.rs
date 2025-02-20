#![crate_type = "cdylib"]
#![no_std]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

extern "Rust" {
    fn print(s: &str);
    fn a() -> i32;
	static HELLO: &'static str;
}

#[no_mangle]
fn b() -> i32 {
    unsafe {
        print("call b()");
		print(HELLO);
        a() + 1
    }
}
