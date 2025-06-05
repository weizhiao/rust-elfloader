#![no_std]
extern crate alloc;

use alloc::string::ToString;
use core::{ffi::c_int, fmt};
use syscalls::{Sysno, raw_syscall};
mod arch;

/// Converts a raw syscall return value to a result.
#[inline(always)]
fn from_ret(value: usize, msg: &str) -> Result<usize, &str> {
    if value > -4096isize as usize {
        // Truncation of the error value is guaranteed to never occur due to
        // the above check. This is the same check that musl uses:
        // https://git.musl-libc.org/cgit/musl/tree/src/internal/syscall_ret.c?h=v1.1.15
        return Err(msg);
    }
    Ok(value)
}

#[inline]
pub fn print_char(c: char) {
    // 将 char 转换为 UTF-8 字节序列（最多 4 字节）
    let mut buffer = [0u8; 4];
    let encoded = c.encode_utf8(&mut buffer);
    let bytes = encoded.as_bytes();

    unsafe {
        from_ret(
            raw_syscall!(Sysno::write, 1, bytes.as_ptr(), bytes.len()),
            "print char failed",
        )
        .unwrap();
    }
}

#[macro_export]
macro_rules! println {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::print(format_args!(concat!($fmt, "\n") $(, $($arg)+)?))
    }
}

pub fn print(args: fmt::Arguments) {
    let s = &args.to_string();
    print_str(s);
}

#[inline]
pub fn print_str(s: &str) {
    unsafe {
        from_ret(
            raw_syscall!(Sysno::write, 1, s.as_ptr(), s.len()),
            "print failed",
        )
        .unwrap();
    }
}

pub fn exit(status: c_int) -> ! {
    unsafe {
        from_ret(raw_syscall!(Sysno::exit, status), "exit failed").unwrap();
    }
    unreachable!()
}
