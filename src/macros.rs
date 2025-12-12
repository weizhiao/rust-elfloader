/// Load a dynamic library into memory
/// # Example
/// ```no_run
/// # use elf_loader::load_dylib;
/// // from file
/// let liba = load_dylib!("target/liba.so");
/// // from memory
/// # let bytes = [];
/// let liba = load_dylib!("liba.so", &bytes);
/// ```
#[macro_export]
macro_rules! load_dylib {
    ($name:expr) => {
        $crate::object::ElfFile::from_path($name).and_then(|file| {
            let mut loader = $crate::Loader::<$crate::mmap::MmapImpl>::new();
            loader.load_dylib(file)
        })
    };
    ($name:expr, $bytes:expr) => {
        $crate::Loader::<$crate::mmap::MmapImpl>::new()
            .load_dylib($crate::object::ElfBinary::new($name, $bytes))
    };
}

/// Load a executable file into memory
/// # Example
/// ```no_run
/// # use elf_loader::load_exec;
/// // from file
/// let liba = load_exec!("target/liba.so");
/// // from memory
/// # let bytes = &[];
/// let liba = load_exec!("liba.so", bytes);
/// ```
#[macro_export]
macro_rules! load_exec {
    ($name:expr) => {
        $crate::object::ElfFile::from_path($name).and_then(|file| {
            let mut loader = $crate::Loader::<$crate::mmap::MmapImpl>::new();
            loader.load_exec(file)
        })
    };
    ($name:expr, $bytes:expr) => {
        $crate::Loader::<$crate::mmap::MmapImpl>::new()
            .load_exec($crate::object::ElfBinary::new($name, $bytes))
    };
}

/// Load a elf file into memory
/// # Example
/// ```no_run
/// # use elf_loader::load;
/// // from file
/// let liba = load!("target/liba.so");
/// // from memory
/// # let bytes = &[];
/// let liba = load!("liba.so", bytes);
/// ```
#[macro_export]
macro_rules! load {
    ($name:expr) => {
        $crate::object::ElfFile::from_path($name).and_then(|file| {
            let mut loader = $crate::Loader::<$crate::mmap::MmapImpl>::new();
            loader.load(file)
        })
    };
    ($name:expr, $bytes:expr) => {
        $crate::Loader::<$crate::mmap::MmapImpl>::new()
            .load($crate::object::ElfBinary::new($name, $bytes))
    };
}
