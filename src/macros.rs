/// Load a dynamic library into memory
/// # Example
/// ```no_run
/// # use elf_loader::load_dylib;
/// // from file
/// let liba = load_dylib!("target/liba.so");
/// // from memory
/// # let bytes = [];
/// let liba = load_dylib!("liba.so", &bytes);
/// // from file with lazy binding
/// let liba = load_dylib!("target/liba.so", lazy: true);
/// // from memory with lazy binding
/// # let bytes = [];
/// let liba = load_dylib!("liba.so", &bytes, lazy: true);
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
    ($name:expr, $bytes:expr, lazy: $lazy:expr) => {
        $crate::Loader::<$crate::mmap::MmapImpl>::new()
            .load_dylib($crate::object::ElfBinary::new($name, $bytes), Some($lazy))
    };
    ($name:expr, lazy: $lazy:expr) => {
        $crate::object::ElfFile::from_path($name).and_then(|file| {
            let mut loader = $crate::Loader::<$crate::mmap::MmapImpl>::new();
            loader.load_dylib(file, Some($lazy))
        })
    };
}

/// Load and relocate a dynamic library in one step
/// # Example
/// ```no_run
/// # use elf_loader::load_and_relocate;
/// # use std::collections::HashMap;
/// # let mut map = HashMap::new();
/// # map.insert("symbol", 0 as *const ());
/// # let pre_find = |name: &str| -> Option<*const ()> { map.get(name).copied() };
/// // from file
/// let liba = load_and_relocate!("target/liba.so", [], &pre_find);
/// // from memory
/// # let bytes = [];
/// let liba = load_and_relocate!("liba.so", &bytes, [], &pre_find);
/// // from file with lazy binding
/// let liba = load_and_relocate!("target/liba.so", [], &pre_find, lazy: true);
/// // from memory with lazy binding
/// # let bytes = [];
/// let liba = load_and_relocate!("liba.so", &bytes, [], &pre_find, lazy: true);
/// ```
#[macro_export]
macro_rules! load_and_relocate {
    ($name:expr, $scope:expr, $pre_find:expr) => {
        $crate::load_dylib!($name).and_then(|lib| {
            lib.easy_relocate($scope, $pre_find)
        })
    };
    ($name:expr, $bytes:expr, $scope:expr, $pre_find:expr) => {
        $crate::load_dylib!($name, $bytes).and_then(|lib| {
            lib.easy_relocate($scope, $pre_find)
        })
    };
    ($name:expr, $scope:expr, $pre_find:expr, lazy: $lazy:expr) => {
        $crate::load_dylib!($name, lazy: $lazy).and_then(|lib| {
            lib.easy_relocate($scope, $pre_find)
        })
    };
    ($name:expr, $bytes:expr, $scope:expr, $pre_find:expr, lazy: $lazy:expr) => {
        $crate::load_dylib!($name, $bytes, lazy: $lazy).and_then(|lib| {
            lib.easy_relocate($scope, $pre_find)
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
    ($name:expr, $bytes:expr) => {
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
    ($name:expr, $bytes:expr) => {
        $crate::Loader::<$crate::mmap::MmapImpl>::new()
            .easy_load($crate::object::ElfBinary::new($name, $bytes))
    };
}
