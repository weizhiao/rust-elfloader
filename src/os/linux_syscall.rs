use crate::object::{ElfFile, ElfObject};
use crate::{Error, io_error};
use crate::{
    Result,
    mmap::{MapFlags, Mmap, ProtFlags},
};
use alloc::borrow::ToOwned;
use alloc::ffi::CString;
use alloc::string::ToString;
use core::ffi::CStr;
use core::str::FromStr;
use core::{
    ffi::{c_int, c_void},
    ptr::NonNull,
};
use syscalls::Sysno;
/// An implementation of Mmap trait
pub struct MmapImpl;

#[inline]
fn mmap(
    addr: *mut c_void,
    len: usize,
    prot: ProtFlags,
    flags: MapFlags,
    fd: c_int,
    offset: isize,
) -> Result<*mut c_void> {
    let ptr = unsafe {
        #[cfg(target_pointer_width = "32")]
        let (syscall, offset) = (Sysno::mmap2, offset / crate::segment::PAGE_SIZE as isize);
        #[cfg(not(target_pointer_width = "32"))]
        let syscall = Sysno::mmap;
        from_ret(
            syscalls::raw_syscall!(syscall, addr, len, prot.bits(), flags.bits(), fd, offset),
            "mmap failed",
        )?
    };
    Ok(ptr as *mut c_void)
}

#[inline]
fn mmap_anonymous(
    addr: *mut c_void,
    len: usize,
    prot: ProtFlags,
    flags: MapFlags,
) -> Result<*mut c_void> {
    let ptr = unsafe {
        #[cfg(target_pointer_width = "32")]
        let syscall = Sysno::mmap2;
        #[cfg(not(target_pointer_width = "32"))]
        let syscall = Sysno::mmap;
        from_ret(
            syscalls::raw_syscall!(
                syscall,
                addr,
                len,
                prot.bits(),
                flags.union(MapFlags::MAP_ANONYMOUS).bits(),
                usize::MAX,
                0
            ),
            "mmap anonymous",
        )?
    };
    Ok(ptr as *mut c_void)
}

#[inline]
fn munmap(addr: *mut c_void, len: usize) -> Result<()> {
    unsafe {
        from_ret(
            syscalls::raw_syscall!(Sysno::munmap, addr, len),
            "munmap failed",
        )?;
    }
    Ok(())
}

#[inline]
fn mprotect(addr: *mut c_void, len: usize, prot: ProtFlags) -> Result<()> {
    unsafe {
        from_ret(
            syscalls::raw_syscall!(Sysno::mprotect, addr, len, prot.bits()),
            "mprotect failed",
        )?;
    }
    Ok(())
}

impl Mmap for MmapImpl {
    unsafe fn mmap(
        addr: Option<usize>,
        len: usize,
        prot: ProtFlags,
        flags: MapFlags,
        offset: usize,
        fd: Option<i32>,
        need_copy: &mut bool,
    ) -> crate::Result<core::ptr::NonNull<core::ffi::c_void>> {
        let ptr = if let Some(fd) = fd {
            mmap(addr.unwrap_or(0) as _, len, prot, flags, fd, offset as _)?
        } else {
            *need_copy = true;
            if let Some(addr) = addr {
                addr as _
            } else {
                mmap_anonymous(0 as _, len, ProtFlags::PROT_WRITE, flags)?
            }
        };
        Ok(unsafe { NonNull::new_unchecked(ptr) })
    }

    unsafe fn mmap_anonymous(
        addr: usize,
        len: usize,
        prot: ProtFlags,
        flags: MapFlags,
    ) -> crate::Result<core::ptr::NonNull<core::ffi::c_void>> {
        let ptr = mmap_anonymous(addr as _, len, prot, flags)?;
        Ok(unsafe { NonNull::new_unchecked(ptr) })
    }

    unsafe fn munmap(addr: core::ptr::NonNull<core::ffi::c_void>, len: usize) -> crate::Result<()> {
        munmap(addr.as_ptr(), len)?;
        Ok(())
    }

    unsafe fn mprotect(
        addr: core::ptr::NonNull<core::ffi::c_void>,
        len: usize,
        prot: ProtFlags,
    ) -> crate::Result<()> {
        mprotect(addr.as_ptr(), len, prot)?;
        Ok(())
    }
}

/// Converts a raw syscall return value to a result.
#[inline(always)]
fn from_ret(value: usize, msg: &str) -> Result<usize> {
    if value > -4096isize as usize {
        // Truncation of the error value is guaranteed to never occur due to
        // the above check. This is the same check that musl uses:
        // https://git.musl-libc.org/cgit/musl/tree/src/internal/syscall_ret.c?h=v1.1.15
        return Err(map_error(msg));
    }
    Ok(value)
}

#[cold]
#[inline(never)]
fn map_error(msg: &str) -> Error {
    Error::MmapError {
        msg: msg.to_string(),
    }
}

pub(crate) fn from_path(path: &str) -> Result<ElfFile> {
    const RDONLY: u32 = 0;
    let name = CString::from_str(path).unwrap().to_owned();
    #[cfg(not(any(target_arch = "aarch64", target_arch = "riscv64")))]
    let fd = unsafe {
        from_io_ret(
            syscalls::raw_syscall!(Sysno::open, name.as_ptr(), RDONLY, 0),
            "open failed",
        )?
    };
    #[cfg(any(target_arch = "aarch64", target_arch = "riscv64"))]
    let fd = unsafe {
        const AT_FDCWD: core::ffi::c_int = -100;
        from_io_ret(
            syscalls::raw_syscall!(Sysno::openat, AT_FDCWD, name.as_ptr(), RDONLY, 0),
            "openat failed",
        )?
    };
    Ok(ElfFile { fd: fd as _, name })
}

impl Drop for ElfFile {
    fn drop(&mut self) {
        unsafe {
            from_io_ret(
                syscalls::raw_syscall!(Sysno::close, self.fd),
                "close failed",
            )
            .unwrap();
        }
    }
}

impl ElfObject for ElfFile {
    fn read(&mut self, buf: &mut [u8], offset: usize) -> Result<()> {
        const SEEK_START: u32 = 0;
        unsafe {
            from_io_ret(
                syscalls::raw_syscall!(Sysno::lseek, self.fd, offset, SEEK_START),
                "lseek failed",
            )?;
            let size = from_io_ret(
                syscalls::raw_syscall!(Sysno::read, self.fd, buf.as_mut_ptr(), buf.len()),
                "read failed",
            )?;
            assert!(size == buf.len());
        }
        Ok(())
    }

    fn file_name(&self) -> &CStr {
        &self.name
    }

    fn as_fd(&self) -> Option<i32> {
        Some(self.fd)
    }
}
/// Converts a raw syscall return value to a result.
#[inline(always)]
fn from_io_ret(value: usize, msg: &str) -> Result<usize> {
    if value > -4096isize as usize {
        // Truncation of the error value is guaranteed to never occur due to
        // the above check. This is the same check that musl uses:
        // https://git.musl-libc.org/cgit/musl/tree/src/internal/syscall_ret.c?h=v1.1.15
        return Err(io_error(msg));
    }
    Ok(value)
}
