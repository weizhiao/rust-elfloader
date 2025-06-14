use crate::{
    Error, Result,
    mmap::{MapFlags, Mmap, ProtFlags},
    object::ElfFile,
};
use alloc::format;
use core::{
    ffi::c_void,
    mem::MaybeUninit,
    ptr::{NonNull, null},
};
use windows_sys::Win32::{
    Foundation::GetLastError,
    System::Memory::{
        self, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE, PAGE_EXECUTE_READ,
        PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, PAGE_READONLY, PAGE_READWRITE,
    },
};

pub struct MmapImpl;

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

impl Mmap for MmapImpl {
    unsafe fn mmap(
        addr: Option<usize>,
        len: usize,
        _prot: ProtFlags,
        _flags: MapFlags,
        _offset: usize,
        fd: Option<i32>,
        need_copy: &mut bool,
    ) -> Result<NonNull<c_void>> {
        assert!(fd.is_none());
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
        prot: ProtFlags,
        _flags: MapFlags,
    ) -> Result<NonNull<c_void>> {
        let ptr = unsafe { Memory::VirtualAlloc(addr as _, len, MEM_COMMIT, prot_win(prot)) };
        Ok(NonNull::new(ptr).unwrap())
    }

    unsafe fn munmap(addr: NonNull<c_void>, _len: usize) -> Result<()> {
        if unsafe { Memory::VirtualFree(addr.as_ptr(), 0, MEM_RELEASE) } == 0 {
            let err_code = unsafe { GetLastError() };
            return Err(Error::MmapError {
                msg: format!("munmap error! error code: {}", err_code),
            });
        }
        Ok(())
    }

    unsafe fn mprotect(addr: NonNull<c_void>, len: usize, prot: ProtFlags) -> Result<()> {
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

pub(crate) fn from_path(path: &str) -> Result<ElfFile> {
    todo!()
}
