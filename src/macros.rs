
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
            loader.easy_load_dylib(file)
        })
    };
    ($name:expr, $bytes:expr) => {
        $crate::Loader::<$crate::mmap::MmapImpl>::new()
            .easy_load_dylib($crate::object::ElfBinary::new($name, $bytes))
    };
    ($name:expr, $bytes:expr, lazy : $lazy:expr) => {
        $crate::Loader::<$crate::mmap::MmapImpl>::new()
            .load_dylib($crate::object::ElfBinary::new($name, $bytes), Some($lazy))
    };
    ($name:expr, lazy : $lazy:expr) => {
        $crate::object::ElfFile::from_path($name).and_then(|file| {
            let mut loader = $crate::Loader::<$crate::mmap::MmapImpl>::new();
            loader.load_dylib(file, Some($lazy))
        })
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
            loader.easy_load_exec(file)
        })
    };
    ($name:expr,$bytes:expr) => {
        $crate::Loader::<$crate::mmap::MmapImpl>::new()
            .easy_load_exec($crate::object::ElfBinary::new($name, $bytes))
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
            loader.easy_load(file)
        })
    };
    ($name:expr,$bytes:expr) => {
        $crate::Loader::<$crate::mmap::MmapImpl>::new()
            .easy_load($crate::object::ElfBinary::new($name, $bytes))
    };
}
