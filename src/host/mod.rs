//! Import libraries managed by the Linux/glibc loader into a Relink scope.
//!
//! Available with `libc` on Linux GNU targets; does not require `std`. Imports
//! retain the native handle and expose global/weak function, object and TLS
//! exports, including GNU symbol versions. IFUNCs are resolved by glibc.
//! Libraries are opened locally and are never added to a Relink scope implicitly.
//!
//! ```no_run
//! use elf_loader::{AddressSpaceId, Domain, Result, host};
//! use elf_loader::tls::DefaultTlsResolver;
//!
//! # fn main() -> Result<()> {
//! let domain = Domain::native(AddressSpaceId::PROCESS)
//!     .with_tls(DefaultTlsResolver::new())?;
//! let module = unsafe { host::open(&domain, "/path/to/libhost.so", "libhost.so")? };
//! // Pass `module` to RelocatorRun::with_modules or insert it in a LinkContext.
//! # Ok(())
//! # }
//! ```

mod error;
mod file;
mod native;

pub use error::HostError;
use error::metadata;
use file::FileExports;
use native::NativeLibrary;

use crate::{
    Domain, IoError, Result,
    arch::NativeArch,
    elf::{ElfSymbolBind, ElfSymbolType},
    image::{ModuleHandle, SyntheticModule, SyntheticSymbol},
    input::{ElfFile, Path},
    relocation::RelocationArch,
    sync::Arc,
    tls::{DefaultTlsResolver, TlsContext, TlsIndex, TlsModuleId, TlsProvider},
};
use alloc::{ffi::CString, string::String};
use core::{ffi::c_void, ptr::NonNull};

/// Opens a native library and imports its exports into `domain` under `name`.
///
/// Uses `RTLD_NOW | RTLD_LOCAL`; the returned module retains the native handle.
/// glibc manages initialization, finalization, and TLS storage. Imported addresses
/// refer to the current process and must be interpreted accordingly by `domain`.
/// Static TLS offsets are not imported, so initial-exec relocations against these
/// TLS exports are unsupported. Symbol versions are retained without the `version`
/// feature; exact version lookup requires that feature.
///
/// Paths are used without SONAME searching. Relative paths, including bare
/// filenames, are relative to the current directory. Export metadata is read
/// through program headers; section headers are optional.
///
/// # Safety
/// Loading, resolving IFUNCs, and unloading can execute arbitrary native code.
/// That code must be safe to execute for the lifetime of the returned module.
/// The file must describe the image returned by `dlopen`, including when glibc
/// reuses an existing mapping. Do not replace or modify the file, or change the
/// working directory, while this call is in progress.
pub unsafe fn open(
    domain: &Domain<NativeArch, DefaultTlsResolver>,
    path: impl AsRef<Path>,
    name: impl Into<String>,
) -> Result<ModuleHandle> {
    let path = path.as_ref();
    let file = ElfFile::from_path(path)?;
    let exports = FileExports::read(&file)?;
    let path = if path.is_absolute() {
        path.into()
    } else {
        Path::new(".").join(path)
    };
    let path = CString::new(path.as_bytes()).map_err(|_| IoError::NullByteInPath)?;
    let library = Arc::new(unsafe { NativeLibrary::open(&path) }?);
    let mut module = SyntheticModule::empty(domain, name).with_data(library.clone());
    if let Some(info) = exports.tls {
        let provider = HostTls::new(library.clone())?;
        // SAFETY: the metadata describes the retained native image; the provider
        // delegates storage to glibc and never frees or republishes its bytes.
        let registration = unsafe {
            domain
                .tls_resolver()
                .register_provider(info, None, provider)
        }?;
        module = module.with_tls(registration)?;
    }
    for file::Export {
        symbol,
        name,
        version,
    } in exports.symbols
    {
        let mut export = if symbol.symbol_type() == ElfSymbolType::TLS {
            SyntheticSymbol::tls(name.as_c_str(), symbol.st_value(), symbol.st_size())
        } else {
            let address =
                library.lookup(&name, version.as_ref().map(|(name, _)| name.as_c_str()))?;
            if symbol.symbol_type() == ElfSymbolType::OBJECT {
                SyntheticSymbol::object(name.as_c_str(), address.cast(), symbol.st_size())
            } else {
                SyntheticSymbol::function(name.as_c_str(), address.cast())
                    .with_size(symbol.st_size())
            }
        }
        .with_other(symbol.st_other());
        if symbol.is_weak() {
            export = export.with_bind(ElfSymbolBind::WEAK);
        }
        if let Some((version, default)) = version {
            export = export.with_version(version.to_bytes(), default);
        }
        module.insert(export);
    }
    Ok(module.into())
}

type TlsGetAddr = unsafe extern "C" fn(*const TlsIndex) -> *mut u8;

struct HostTls {
    library: Arc<NativeLibrary>,
    mod_id: TlsModuleId,
    get_addr: TlsGetAddr,
}

impl HostTls {
    fn new(library: Arc<NativeLibrary>) -> Result<Self> {
        let mut id = 0usize;
        let status = unsafe {
            libc::dlinfo(
                library.handle(),
                libc::RTLD_DI_TLS_MODID,
                (&mut id as *mut usize).cast(),
            )
        };
        if status != 0 {
            return Err(native::last_error("dlinfo(RTLD_DI_TLS_MODID)"));
        }
        if id == 0 {
            return Err(metadata("PT_TLS has no native module ID"));
        }
        let address = library.lookup(c"__tls_get_addr", None)?;
        if address.is_null() {
            return Err(metadata("null __tls_get_addr entry"));
        }
        // SAFETY: the native ABI defines this entry with the TlsGetAddr signature.
        let get_addr = unsafe { core::mem::transmute::<*mut c_void, TlsGetAddr>(address) };
        Ok(Self {
            library,
            mod_id: TlsModuleId::new(id),
            get_addr,
        })
    }
}

// SAFETY: the retained handle pins glibc's registration. glibc supplies each
// thread's block; we translate IDs and the architecture's DTV bias at the boundary.
unsafe impl TlsProvider for HostTls {
    unsafe fn resolve(&self, _context: &TlsContext, offset: usize) -> Result<NonNull<u8>> {
        let index = TlsIndex {
            ti_module: self.mod_id,
            ti_offset: offset.wrapping_sub(NativeArch::TLS_DTV_OFFSET),
        };
        NonNull::new(unsafe { (self.get_addr)(&index) }).ok_or_else(|| {
            crate::TlsError::InvalidModuleId {
                mod_id: self.mod_id,
            }
            .into()
        })
    }

    unsafe fn get(&self, _context: &TlsContext) -> Option<NonNull<u8>> {
        let mut data: *mut u8 = core::ptr::null_mut();
        let status = unsafe {
            libc::dlinfo(
                self.library.handle(),
                libc::RTLD_DI_TLS_DATA,
                (&mut data as *mut *mut u8).cast(),
            )
        };
        (status == 0).then(|| NonNull::new(data)).flatten()
    }
}
