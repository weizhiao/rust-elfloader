//! The original elf object
use crate::Result;
use core::ffi::CStr;
mod binary;
#[cfg(feature = "fs")]
mod file;

pub use binary::ElfBinary;
#[cfg(feature = "fs")]
pub use file::ElfFile;

/// The original elf object
pub trait ElfObject {
    /// Returns the elf object name
    fn file_name(&self) -> &CStr;
    /// Read data from the elf object
    fn read(&mut self, buf: &mut [u8], offset: usize) -> Result<()>;
    /// Extracts the raw file descriptor.
    fn as_fd(&self) -> Option<i32>;
}

/// The original elf object
pub trait ElfObjectAsync: ElfObject {
    /// Read data from the elf object
    fn read_async(
        &mut self,
        buf: &mut [u8],
        offset: usize,
    ) -> impl core::future::Future<Output = Result<()>> + Send;
}
