use crate::Result;
use alloc::ffi::CString;

/// An elf file saved in a file
pub struct ElfFile {
    name: CString,
    fd: i32,
}

impl ElfFile {
    /// # Safety
    ///
    /// The `fd` passed in must be an owned file descriptor; in particular, it must be open.
    pub unsafe fn from_owned_fd(path: &str, raw_fd: i32) -> Self {
        ElfFile {
            name: CString::new(path).unwrap(),
            fd: raw_fd,
        }
    }

    pub fn from_path(path: &str) -> Result<Self> {
        from_path(path)
    }
}

#[cfg(feature = "use-libc")]
mod imp {
    use super::ElfFile;
    use crate::{Result, io_error, object::ElfObject};
    use alloc::ffi::CString;
    use core::{ffi::CStr, str::FromStr};
    use libc::{O_RDONLY, SEEK_SET};

    impl Drop for ElfFile {
        fn drop(&mut self) {
            unsafe { libc::close(self.fd) };
        }
    }

    pub(crate) fn from_path(path: &str) -> Result<ElfFile> {
        let name = CString::from_str(path).unwrap();
        let fd = unsafe { libc::open(name.as_ptr(), O_RDONLY) };
        if fd == -1 {
            return Err(io_error("open failed"));
        }
        Ok(ElfFile { name, fd })
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

    impl ElfObject for ElfFile {
        fn read(&mut self, buf: &mut [u8], offset: usize) -> Result<()> {
            lseek(self.fd, offset)?;
            read_exact(self.fd, buf)?;
            Ok(())
        }

        fn file_name(&self) -> &CStr {
            &self.name
        }

        fn as_fd(&self) -> Option<i32> {
            Some(self.fd)
        }
    }
}

#[cfg(feature = "use-syscall")]
mod imp {
    use super::ElfFile;
    use crate::{Result, io_error, object::ElfObject};
    use alloc::{borrow::ToOwned, ffi::CString};
    use core::{ffi::CStr, str::FromStr};
    use syscalls::Sysno;

    pub(crate) fn from_path(path: &str) -> Result<ElfFile> {
        const RDONLY: u32 = 0;
        let name = CString::from_str(path).unwrap().to_owned();
        #[cfg(not(any(target_arch = "aarch64", target_arch = "riscv64")))]
        let fd = unsafe {
            from_ret(
                syscalls::raw_syscall!(Sysno::open, name.as_ptr(), RDONLY, 0),
                "open failed",
            )?
        };
        #[cfg(any(target_arch = "aarch64", target_arch = "riscv64"))]
        let fd = unsafe {
            const AT_FDCWD: core::ffi::c_int = -100;
            from_ret(
                syscalls::raw_syscall!(Sysno::openat, AT_FDCWD, name.as_ptr(), RDONLY, 0),
                "openat failed",
            )?
        };
        Ok(ElfFile { fd: fd as _, name })
    }

    impl Drop for ElfFile {
        fn drop(&mut self) {
            unsafe {
                from_ret(
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
                from_ret(
                    syscalls::raw_syscall!(Sysno::lseek, self.fd, offset, SEEK_START),
                    "lseek failed",
                )?;
                let size = from_ret(
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
    fn from_ret(value: usize, msg: &str) -> Result<usize> {
        if value > -4096isize as usize {
            // Truncation of the error value is guaranteed to never occur due to
            // the above check. This is the same check that musl uses:
            // https://git.musl-libc.org/cgit/musl/tree/src/internal/syscall_ret.c?h=v1.1.15
            return Err(io_error(msg));
        }
        Ok(value)
    }
}

use imp::from_path;
