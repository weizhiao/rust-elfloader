use crate::{
    Error, Result, io_error,
    mmap::{MapFlags, Mmap, ProtFlags},
    object::{ElfFile, ElfObject},
};
use alloc::{ffi::CString, format, string::ToString};
use core::{
    ffi::{CStr, c_void},
    mem::{self, MaybeUninit},
    ptr::{NonNull, null, null_mut},
    str::FromStr,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, HANDLE, TRUE},
    Security::SECURITY_ATTRIBUTES,
    Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_BEGIN, FILE_SHARE_READ, GENERIC_READ,
        OPEN_EXISTING, ReadFile, SetFilePointerEx,
    },
    System::Memory::{
        self as Memory, CreateFileMappingW, FILE_MAP_ALL_ACCESS, FILE_MAP_EXECUTE, FILE_MAP_READ,
        FILE_MAP_WRITE, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, MapViewOfFileEx, PAGE_EXECUTE,
        PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, PAGE_READONLY,
        PAGE_READWRITE,
    },
};

pub struct MmapImpl;

fn prot_win(prot: ProtFlags, is_create_file_mapping: bool) -> PAGE_PROTECTION_FLAGS {
    match prot.bits() {
        1 => PAGE_READONLY,
        0b10 | 0b11 => PAGE_READWRITE,
        // PAGE_EXECUTE is not supported by the CreateFileMapping function:
        // https://learn.microsoft.com/en-us/windows/win32/memory/memory-protection-constants.
        0b100 => {
            if is_create_file_mapping {
                PAGE_EXECUTE_READ
            } else {
                PAGE_EXECUTE
            }
        }
        0b101 => PAGE_EXECUTE_READ,
        0b111 => PAGE_EXECUTE_READWRITE,
        _ => {
            panic!("Unsupported protection flags");
        }
    }
}

fn prot_to_file_map_access(prot: ProtFlags) -> u32 {
    match prot.bits() {
        1 => FILE_MAP_READ,
        0b10 | 0b11 => FILE_MAP_READ | FILE_MAP_WRITE,
        0b100 => FILE_MAP_READ | FILE_MAP_EXECUTE,
        0b101 => FILE_MAP_READ | FILE_MAP_EXECUTE,
        0b111 => FILE_MAP_ALL_ACCESS,
        _ => FILE_MAP_READ,
    }
}

impl Mmap for MmapImpl {
    unsafe fn mmap(
        addr: Option<usize>,
        len: usize,
        prot: ProtFlags,
        flags: MapFlags,
        offset: usize,
        fd: Option<isize>,
        need_copy: &mut bool,
    ) -> Result<NonNull<c_void>> {
        let ptr = if let Some(fd) = fd {
            let handle = fd as HANDLE;
            let mut sa = SECURITY_ATTRIBUTES {
                nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: null_mut(),
                bInheritHandle: TRUE,
            };

            let mapping_handle = unsafe {
                CreateFileMappingW(
                    handle,
                    &mut sa,
                    prot_win(prot, true),
                    (len >> 32) as u32,
                    (len & 0xFFFFFFFF) as u32,
                    null(),
                )
            };
            if mapping_handle == 0 {
                let err_code = unsafe { GetLastError() };
                return Err(Error::MmapError {
                    msg: format!("CreateFileMappingW failed with error: {}", err_code),
                });
            }

            let desired_addr = addr.unwrap_or(0) as *mut c_void;
            let file_offset_high = (offset >> 32) as u32;
            let file_offset_low = (offset & 0xFFFFFFFF) as u32;

            let ptr = unsafe {
                MapViewOfFileEx(
                    mapping_handle,
                    prot_to_file_map_access(prot),
                    file_offset_high,
                    file_offset_low,
                    len,
                    desired_addr,
                )
            };

            unsafe { CloseHandle(mapping_handle) };

            if ptr.Value.is_null() {
                let err_code = unsafe { GetLastError() };
                return Err(Error::MmapError {
                    msg: format!("MapViewOfFileEx failed with error: {}", err_code),
                });
            }

            ptr.Value
        } else {
            *need_copy = true;
            if let Some(addr) = addr {
                addr as _
            } else {
                unsafe {
                    let ptr = Memory::VirtualAlloc(
                        null(),
                        len,
                        MEM_RESERVE | MEM_COMMIT,
                        prot_win(ProtFlags::PROT_WRITE, false),
                    );
                    ptr
                }
            };
        };
        Ok(NonNull::new(ptr).unwrap())
    }

    unsafe fn mmap_anonymous(
        addr: usize,
        len: usize,
        prot: ProtFlags,
        _flags: MapFlags,
    ) -> Result<NonNull<c_void>> {
        let ptr =
            unsafe { Memory::VirtualAlloc(addr as _, len, MEM_COMMIT, prot_win(prot, false)) };
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
        if unsafe {
            Memory::VirtualProtect(addr.as_ptr(), len, prot_win(prot, false), old.as_mut_ptr())
        } == 0
        {
            let err_code = unsafe { GetLastError() };
            return Err(Error::MmapError {
                msg: format!("mprotect error! error code: {}", err_code),
            });
        }
        Ok(())
    }
}

impl Drop for ElfFile {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.fd as HANDLE) };
    }
}

pub(crate) fn from_path(path: &str) -> Result<ElfFile> {
    let mut wide_path = Vec::<u16>::with_capacity(path.len() + 1);
    for c in path.encode_utf16() {
        wide_path.push(c);
    }
    wide_path.push(0);

    let handle = unsafe {
        CreateFileW(
            wide_path.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ,
            null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            0,
        )
    };

    if handle == 0 {
        let err_code = unsafe { GetLastError() };
        return Err(io_error(&format!(
            "CreateFileW failed with error: {}",
            err_code
        )));
    }

    Ok(ElfFile {
        name: CString::from_str(path).unwrap(),
        fd: handle as isize,
    })
}

fn win_seek(handle: HANDLE, offset: usize) -> Result<()> {
    let distance = offset as i64;
    let mut new_pos = 0i64;

    let res = unsafe { SetFilePointerEx(handle, distance, &mut new_pos, FILE_BEGIN) };

    if res == 0 || new_pos as usize != offset {
        let err_code = unsafe { GetLastError() };
        return Err(io_error(&format!(
            "SetFilePointerEx failed with error: {}",
            err_code
        )));
    }
    Ok(())
}

fn win_read_exact(handle: HANDLE, mut bytes: &mut [u8]) -> Result<()> {
    let mut bytes_read = 0;

    loop {
        if bytes.is_empty() {
            return Ok(());
        }

        let bytes_to_read = bytes.len().min(u32::MAX as usize) as u32;
        let ptr = bytes.as_mut_ptr();
        let mut read_count = 0u32;

        let result = unsafe {
            ReadFile(
                handle,
                ptr as *mut c_void,
                bytes_to_read,
                &mut read_count,
                null_mut(),
            )
        };

        if result == 0 {
            let err_code = unsafe { GetLastError() };
            return Err(io_error(&format!(
                "ReadFile failed with error: {}",
                err_code
            )));
        } else if read_count == 0 {
            return Err(io_error("failed to fill buffer"));
        }

        let n = read_count as usize;
        bytes_read += n;

        bytes = &mut bytes[n..];
    }
}

impl ElfObject for ElfFile {
    fn read(&mut self, buf: &mut [u8], offset: usize) -> Result<()> {
        win_seek(self.fd as HANDLE, offset)?;
        win_read_exact(self.fd as HANDLE, buf)?;
        Ok(())
    }

    fn file_name(&self) -> &CStr {
        &self.name
    }

    fn as_fd(&self) -> Option<isize> {
        Some(self.fd)
    }
}
