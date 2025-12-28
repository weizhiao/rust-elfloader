use crate::{
    Error, Result, io_error,
    os::{MapFlags, Mmap, ProtFlags},
    input::ElfReader,
};
use alloc::{
    ffi::CString,
    string::{String, ToString},
};
use core::{ffi::c_void, ptr::NonNull, str::FromStr};
use libc::{O_RDONLY, SEEK_SET, mmap, mprotect, munmap};

/// An implementation of Mmap trait
pub struct DefaultMmap;

pub(crate) struct RawFile {
    name: String,
    fd: isize,
}

impl Mmap for DefaultMmap {
    unsafe fn mmap(
        addr: Option<usize>,
        len: usize,
        prot: ProtFlags,
        flags: MapFlags,
        offset: usize,
        fd: Option<isize>,
        need_copy: &mut bool,
    ) -> crate::Result<core::ptr::NonNull<c_void>> {
        let ptr = if let Some(fd) = fd {
            unsafe {
                mmap(
                    addr.unwrap_or(0) as _,
                    len,
                    prot.bits(),
                    flags.bits(),
                    fd as i32,
                    offset as _,
                )
            }
        } else {
            *need_copy = true;
            addr.unwrap() as _
        };
        if core::ptr::eq(ptr, libc::MAP_FAILED) {
            return Err(map_error("mmap failed"));
        }
        Ok(unsafe { NonNull::new_unchecked(ptr) })
    }

    unsafe fn mmap_anonymous(
        addr: usize,
        len: usize,
        prot: ProtFlags,
        flags: MapFlags,
    ) -> crate::Result<core::ptr::NonNull<c_void>> {
        let ptr = unsafe {
            mmap(
                addr as _,
                len,
                prot.bits(),
                flags.union(MapFlags::MAP_ANONYMOUS).bits(),
                -1,
                0,
            )
        };
        if core::ptr::eq(ptr, libc::MAP_FAILED) {
            return Err(map_error("mmap anonymous failed"));
        }
        Ok(unsafe { NonNull::new_unchecked(ptr) })
    }

    unsafe fn munmap(addr: core::ptr::NonNull<core::ffi::c_void>, len: usize) -> crate::Result<()> {
        let res = unsafe { munmap(addr.as_ptr(), len) };
        if res != 0 {
            return Err(map_error("munmap failed"));
        }
        Ok(())
    }

    unsafe fn mprotect(
        addr: core::ptr::NonNull<core::ffi::c_void>,
        len: usize,
        prot: ProtFlags,
    ) -> crate::Result<()> {
        let res = unsafe { mprotect(addr.as_ptr(), len, prot.bits()) };
        if res != 0 {
            return Err(map_error("mprotect failed"));
        }
        Ok(())
    }

    unsafe fn mmap_reserve(
        addr: Option<usize>,
        len: usize,
        use_file: bool,
    ) -> Result<NonNull<c_void>> {
        let flags = MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS;
        let prot = if use_file {
            ProtFlags::PROT_NONE
        } else {
            ProtFlags::PROT_WRITE
        };
        let ptr = unsafe {
            mmap(
                addr.unwrap_or(0) as _,
                len,
                prot.bits(),
                flags.bits(),
                -1,
                0,
            )
        };
        Ok(unsafe { NonNull::new_unchecked(ptr) })
    }
}

impl Drop for RawFile {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd as i32) };
    }
}

impl RawFile {
    pub(crate) fn from_path(path: &str) -> Result<Self> {
        let name = CString::from_str(path).unwrap();
        let fd = unsafe { libc::open(name.as_ptr(), O_RDONLY) };
        if fd == -1 {
            return Err(io_error("open failed"));
        }
        Ok(Self {
            name: path.to_string(),
            fd: fd as isize,
        })
    }

    pub(crate) fn from_owned_fd(path: &str, raw_fd: i32) -> Self {
        Self {
            name: path.to_string(),
            fd: raw_fd as isize,
        }
    }
}

fn lseek(fd: i32, offset: usize) -> Result<()> {
    let off = unsafe { libc::lseek(fd, offset as _, SEEK_SET) };
    if off == -1 || off as usize != offset {
        return Err(io_error("lseek failed"));
    }
    Ok(())
}

fn read_exact(fd: i32, mut bytes: &mut [u8]) -> Result<()> {
    loop {
        if bytes.is_empty() {
            return Ok(());
        }
        // 尝试读取剩余的字节数
        let bytes_to_read = bytes.len();
        let ptr = bytes.as_mut_ptr() as *mut libc::c_void;
        let result = unsafe { libc::read(fd, ptr, bytes_to_read) };

        if result < 0 {
            // 出现错误
            return Err(io_error("read error"));
        } else if result == 0 {
            // 意外到达文件末尾
            return Err(io_error("failed to fill buffer"));
        }
        // 成功读取了部分字节
        let n = result as usize;
        // 更新剩余需要读取的部分
        bytes = &mut bytes[n..];
    }
}

impl ElfReader for RawFile {
    fn read(&mut self, buf: &mut [u8], offset: usize) -> Result<()> {
        lseek(self.fd as i32, offset)?;
        read_exact(self.fd as i32, buf)?;
        Ok(())
    }

    fn file_name(&self) -> &str {
        &self.name
    }

    fn as_fd(&self) -> Option<isize> {
        Some(self.fd)
    }
}

#[cold]
#[inline(never)]
fn map_error(msg: &str) -> Error {
    Error::Mmap {
        msg: msg.to_string().into(),
    }
}
