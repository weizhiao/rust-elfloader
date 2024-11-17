use alloc::string::ToString;
use core::{
    ffi::{c_int, c_void},
    fmt,
};
use syscalls::{syscall1, syscall2, syscall3, syscall6, Sysno};

pub const MAP_ANONYMOUS: c_int = 0x0020;
pub const SEEK_SET: c_int = 0;

#[macro_export]
macro_rules! print {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::syscall::print(format_args!($fmt $(, $($arg)+)?))
    }
}

#[macro_export]
macro_rules! println {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::syscall::print(format_args!(concat!($fmt, "\n") $(, $($arg)+)?))
    }
}

pub fn print(args: fmt::Arguments) {
    let s = &args.to_string();
    let _ = unsafe { syscall3(Sysno::write, 1, s.as_ptr() as usize, s.len()) }.unwrap();
}

pub fn mmap(
    addr: *mut c_void,
    len: usize,
    prot: c_int,
    flags: c_int,
    fd: c_int,
    offset: isize,
) -> *mut c_void {
    unsafe {
        syscall6(
            Sysno::mmap,
            addr as usize,
            len,
            prot as _,
            flags as _,
            fd as _,
            offset as _,
        )
        .unwrap() as _
    }
}

pub fn munmap(addr: *mut c_void, len: usize) -> c_int {
    unsafe { syscall2(Sysno::munmap, addr as usize, len) }.unwrap() as _
}

pub fn mprotect(addr: *mut c_void, len: usize, prot: c_int) -> c_int {
    unsafe { syscall3(Sysno::mprotect, addr as usize, len, prot as usize) }.unwrap() as _
}

pub fn lseek(fd: c_int, offset: isize, whence: c_int) -> c_int {
    unsafe { syscall3(Sysno::lseek, fd as usize, offset as usize, whence as usize) }.unwrap() as _
}

// 包装 read 系统调用的函数
pub fn read(fd: c_int, buf: *mut c_void, count: usize) -> c_int {
    unsafe { syscall3(Sysno::read, fd as usize, buf as usize, count).unwrap() as _ }
}

pub fn open(path: *const i8, flags: c_int, mode: c_int) -> c_int {
    unsafe { syscall3(Sysno::open, path as usize, flags as usize, mode as usize).unwrap() as _ }
}

pub fn exit(status: c_int) -> ! {
    unsafe {
        syscall1(Sysno::exit, status as usize).unwrap();
    }
    unreachable!()
}
