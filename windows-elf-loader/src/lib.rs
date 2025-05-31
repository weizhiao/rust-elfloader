use elf_loader::{ElfDylib, Loader, object::ElfBinary};
use mmap::WindowsMmap;
use std::sync::Arc;
mod mmap;

/// elf loader
pub struct WinElfLoader {
    loader: Loader<WindowsMmap>,
}

impl WinElfLoader {
    pub fn new() -> Self {
        let mut loader = Loader::new();
        let sysv_abi = Arc::new(|func: Option<fn()>, func_array: Option<&[fn()]>| {
            func.iter()
                .chain(func_array.unwrap_or(&[]).iter())
                .for_each(
                    |init| unsafe { core::mem::transmute::<_, &extern "sysv64" fn()>(init) }(),
                );
        });
        loader.set_init(sysv_abi.clone());
        loader.set_fini(sysv_abi);
        Self { loader }
    }

    pub fn load_dylib(
        &mut self,
        name: &str,
        bytes: impl AsRef<[u8]>,
    ) -> Result<ElfDylib, elf_loader::Error> {
        let object = ElfBinary::new(name, bytes.as_ref());
        self.loader.load_dylib(object, Some(false))
    }
}
