//! The original elf object
use crate::Result;
use core::ffi::CStr;
mod binary;
#[cfg(feature = "std")]
mod file;
#[cfg(feature = "std")]
mod socket;

pub use binary::ElfBinary;
#[cfg(feature = "std")]
pub use file::ElfFile;
#[cfg(feature = "std")]
pub use socket::ElfStream;

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
pub trait ElfObjectAsync {
    /// Returns the elf object name
    fn file_name(&self) -> &CStr;
    /// Read data from the elf object
    fn read(
        &mut self,
        buf: &mut [u8],
        offset: usize,
    ) -> impl core::future::Future<Output = Result<()>> + Send;
    /// Extracts the raw file descriptor.
    fn as_fd(&self) -> Option<i32>;
}
