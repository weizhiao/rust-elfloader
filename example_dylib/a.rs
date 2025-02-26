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

#[repr(C)]
pub struct S {
    a: u64,
    b: u32,
    c: u16,
    d: u8,
}

#[no_mangle]
pub extern "C" fn test_identity_struct(x: S) -> S {
    x
}

#[no_mangle]
pub static HELLO: &str = "Hello!";
