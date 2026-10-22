use core::{
    ffi::c_void,
    mem::{MaybeUninit, align_of, size_of},
    ptr::{self, NonNull},
};

use alloc::boxed::Box;

use crate::{
    ByteRepr, Result,
    os::{MadviseAdvice, Mmap, ProtFlags},
};

use super::{MappedRegion, RegionAccess, VmAddr};

/// Host-addressable mmap region.
pub struct HostRegion {
    host_ptr: *mut c_void,
    len: usize,
    control: Option<Box<dyn Mmap<Region = HostRegion>>>,
    unmap_on_drop: bool,
}

impl MappedRegion<HostRegion> {
    /// Creates a host-backed mapped region that unmaps through `control` on drop.
    ///
    /// # Safety
    /// `host_ptr..host_ptr + len` must describe a valid, non-overflowing mapping
    /// or address reservation owned by the caller. Ownership is transferred to
    /// the returned region: no other owner may release it, and it must remain
    /// valid until the last clone is dropped. `control` must support operations
    /// on this range and correctly release it through `munmap(host_ptr, len)`.
    /// Reserved pages must be populated with suitable permissions before access.
    #[inline]
    pub unsafe fn local<M>(host_ptr: *mut c_void, len: usize, control: M) -> Self
    where
        M: Mmap<Region = HostRegion>,
    {
        Self::new(HostRegion::with_control(
            host_ptr,
            len,
            Box::new(control),
            true,
        ))
    }

    /// Creates a host-backed mapped region alias that does not unmap on drop.
    ///
    /// # Safety
    /// `host_ptr..host_ptr + len` must describe a valid, non-overflowing mapping
    /// or address reservation. Its owner must keep it valid for the lifetime of
    /// every clone of the returned region. `control` must support operations on
    /// this range. The caller must coordinate accesses and protection changes
    /// with the owner and other aliases; pages need suitable permissions before access.
    #[inline]
    pub unsafe fn local_alias<M>(host_ptr: *mut c_void, len: usize, control: M) -> Self
    where
        M: Mmap<Region = HostRegion>,
    {
        Self::new(HostRegion::with_control(
            host_ptr,
            len,
            Box::new(control),
            false,
        ))
    }

    /// Creates a host-backed alias without taking ownership of the mapping.
    ///
    /// # Safety
    /// `host_ptr..host_ptr + len` must remain mapped and accessible for the
    /// lifetime of every clone of the returned region. The range must not overflow,
    /// and accesses must be coordinated with the owner and other aliases as for
    /// [`Self::local_alias`].
    #[inline]
    pub unsafe fn local_alias_no_unmap(host_ptr: *mut c_void, len: usize) -> Self {
        Self::new(HostRegion {
            host_ptr,
            len,
            control: None,
            unmap_on_drop: false,
        })
    }
}

impl HostRegion {
    #[inline]
    fn with_control(
        host_ptr: *mut c_void,
        len: usize,
        control: Box<dyn Mmap<Region = HostRegion>>,
        unmap_on_drop: bool,
    ) -> Self {
        Self {
            host_ptr,
            len,
            control: Some(control),
            unmap_on_drop,
        }
    }
}

impl RegionAccess for HostRegion {
    #[inline]
    fn addr(&self) -> VmAddr {
        VmAddr::new(self.host_ptr as usize)
    }

    #[inline]
    fn len(&self) -> usize {
        self.len
    }

    #[inline]
    unsafe fn read_bytes(&self, offset: usize, dst: &mut [u8]) -> Result<()> {
        if dst.is_empty() {
            return Ok(());
        }
        unsafe {
            ptr::copy_nonoverlapping(
                self.host_ptr.cast::<u8>().add(offset),
                dst.as_mut_ptr(),
                dst.len(),
            );
        }
        Ok(())
    }

    #[inline]
    unsafe fn write_bytes(&self, offset: usize, src: &[u8]) -> Result<()> {
        if src.is_empty() {
            return Ok(());
        }
        unsafe {
            ptr::copy_nonoverlapping(
                src.as_ptr(),
                self.host_ptr.cast::<u8>().add(offset),
                src.len(),
            );
        }
        Ok(())
    }

    #[inline]
    unsafe fn read_value<T: ByteRepr>(&self, offset: usize) -> Result<T> {
        if size_of::<T>() == 0 {
            return Ok(unsafe { MaybeUninit::<T>::zeroed().assume_init() });
        }
        let ptr = unsafe { self.host_ptr.cast::<u8>().add(offset).cast::<T>() };
        debug_assert!((ptr as usize).is_multiple_of(align_of::<T>()));
        Ok(unsafe { ptr::read(ptr) })
    }

    #[inline]
    unsafe fn write_value<T: ByteRepr>(&self, offset: usize, value: T) -> Result<()> {
        if size_of::<T>() == 0 {
            return Ok(());
        }
        let ptr = unsafe { self.host_ptr.cast::<u8>().add(offset).cast::<T>() };
        debug_assert!((ptr as usize).is_multiple_of(align_of::<T>()));
        unsafe {
            ptr::write(ptr, value);
        }
        Ok(())
    }

    #[inline]
    unsafe fn zero_bytes(&self, offset: usize, len: usize) -> Result<()> {
        if len == 0 {
            return Ok(());
        }
        unsafe {
            ptr::write_bytes(self.host_ptr.cast::<u8>().add(offset), 0, len);
        }
        Ok(())
    }

    #[inline]
    unsafe fn borrow_bytes(&self, offset: usize, len: usize) -> Option<&'static [u8]> {
        if len == 0 {
            return Some(&[]);
        }
        Some(unsafe { core::slice::from_raw_parts(self.host_ptr.cast::<u8>().add(offset), len) })
    }

    #[inline]
    unsafe fn host_ptr(&self, offset: usize) -> Option<NonNull<u8>> {
        Some(unsafe { NonNull::new_unchecked(self.host_ptr.cast::<u8>().add(offset)) })
    }

    #[inline]
    unsafe fn madvise(&self, offset: usize, len: usize, behavior: MadviseAdvice) -> Result<()> {
        let Some(control) = &self.control else {
            return Ok(());
        };
        unsafe {
            control.madvise(
                VmAddr::from_ptr(self.host_ptr.cast::<u8>().add(offset)),
                len,
                behavior,
            )
        }
    }

    #[inline]
    unsafe fn mprotect(&self, offset: usize, len: usize, prot: ProtFlags) -> Result<()> {
        let Some(control) = &self.control else {
            return Ok(());
        };
        unsafe {
            control.mprotect(
                VmAddr::from_ptr(self.host_ptr.cast::<u8>().add(offset)),
                len,
                prot,
            )
        }
    }
}

impl Drop for HostRegion {
    fn drop(&mut self) {
        if self.unmap_on_drop {
            let control = self
                .control
                .as_ref()
                .expect("owned region must have a mapping backend");
            let _ = unsafe { control.munmap(VmAddr::from_ptr(self.host_ptr), self.len) };
        }
    }
}

// Safety: mapped regions operate on an mmap-style allocation and delegate
// synchronization requirements to the mapping backend.
unsafe impl Send for HostRegion {}
// Safety: shared access only exposes byte operations over the mapped range.
unsafe impl Sync for HostRegion {}
