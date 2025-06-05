use std::{
    mem::MaybeUninit,
    ptr::{NonNull, null},
    sync::LazyLock,
};

use elf_loader::{
    Error,
    mmap::{Mmap, ProtFlags},
};
use windows_sys::Win32::{
    Foundation::GetLastError,
    System::{
        Memory::{
            self, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE, PAGE_EXECUTE_READ,
            PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, PAGE_READONLY, PAGE_READWRITE,
        },
        SystemInformation::GetSystemInfo,
    },
};

static PAGE_SIZE: LazyLock<usize> = LazyLock::new(|| {
    let mut sys_info = MaybeUninit::uninit();
    unsafe {
        GetSystemInfo(sys_info.as_mut_ptr());
        sys_info.assume_init().dwPageSize as usize
    }
});

pub struct WindowsMmap;

fn prot_win(prot: ProtFlags) -> PAGE_PROTECTION_FLAGS {
    match prot.bits() {
        1 => PAGE_READONLY,
        0b10 | 0b11 => PAGE_READWRITE,
        0b100 => PAGE_EXECUTE,
        0b101 => PAGE_EXECUTE_READ,
        0b111 => PAGE_EXECUTE_READWRITE,
        _ => {
            panic!();
        }
    }
}

impl Mmap for WindowsMmap {
    unsafe fn mmap(
        addr: Option<usize>,
        len: usize,
        _prot: elf_loader::mmap::ProtFlags,
        _flags: elf_loader::mmap::MapFlags,
        _offset: usize,
        fd: Option<i32>,
        need_copy: &mut bool,
    ) -> elf_loader::Result<std::ptr::NonNull<std::ffi::c_void>> {
        assert!(fd.is_none());
        assert!(*PAGE_SIZE == 4096);
        // Currently not support mmap file
        let ptr = if let Some(addr) = addr {
            addr as _
        } else {
            unsafe {
                let ptr = Memory::VirtualAlloc(
                    null(),
                    len,
                    MEM_RESERVE | MEM_COMMIT,
                    prot_win(ProtFlags::PROT_WRITE),
                );
                ptr
            }
        };
        *need_copy = true;
        Ok(NonNull::new(ptr).unwrap())
    }

    unsafe fn mmap_anonymous(
        addr: usize,
        len: usize,
        prot: elf_loader::mmap::ProtFlags,
        _flags: elf_loader::mmap::MapFlags,
    ) -> elf_loader::Result<std::ptr::NonNull<std::ffi::c_void>> {
        let ptr = unsafe { Memory::VirtualAlloc(addr as _, len, MEM_COMMIT, prot_win(prot)) };
        Ok(NonNull::new(ptr).unwrap())
    }

    unsafe fn munmap(
        addr: std::ptr::NonNull<std::ffi::c_void>,
        _len: usize,
    ) -> elf_loader::Result<()> {
        if unsafe { Memory::VirtualFree(addr.as_ptr(), 0, MEM_RELEASE) } == 0 {
            let err_code = unsafe { GetLastError() };
            return Err(Error::MmapError {
                msg: format!("munmap error! error code: {}", err_code),
            });
        }
        Ok(())
    }

    unsafe fn mprotect(
        addr: std::ptr::NonNull<std::ffi::c_void>,
        len: usize,
        prot: elf_loader::mmap::ProtFlags,
    ) -> elf_loader::Result<()> {
        let mut old = MaybeUninit::uninit();
        if unsafe { Memory::VirtualProtect(addr.as_ptr(), len, prot_win(prot), old.as_mut_ptr()) }
            == 0
        {
            let err_code = unsafe { GetLastError() };
            return Err(Error::MmapError {
                msg: format!("mprotect error! error code: {}", err_code),
            });
        }
        Ok(())
    }
}
