use elf_loader::{Loader, mmap::MmapImpl, object::ElfFile};

fn main() {
    unsafe { std::env::set_var("RUST_LOG", "trace") };
    env_logger::init();
    // Load and relocate dynamic library liba.so
    let mut loader = Loader::<MmapImpl>::new();
    let object = ElfFile::from_path("a.o").unwrap();
    loader.load_relocatable(object).unwrap();
}
