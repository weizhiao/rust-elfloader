#[path = "support/mod.rs"]
mod fixture_support;

use elf_loader::{
    AddressSpaceId, Domain, Loader, Relocator, Result,
    image::{ModuleHandle, SyntheticModule, SyntheticSymbol},
};

const LOADER: Loader = Loader::new();
const RELOCATOR: Relocator = Relocator::new();

fn host_symbols(domain: &Domain) -> ModuleHandle {
    fn print(s: &str) {
        println!("{}", s);
    }

    SyntheticModule::new(
        domain,
        "__host",
        [SyntheticSymbol::function(c"print", print as *const ())],
    )
    .into()
}

fn main() -> Result<()> {
    unsafe { std::env::set_var("RUST_LOG", "trace") };
    env_logger::init();

    let fixtures = fixture_support::ensure_all();
    let domain = Domain::native(AddressSpaceId::PROCESS);
    let host = host_symbols(&domain);
    let mut loader = LOADER.run(domain);
    let base = RELOCATOR
        .run(loader.load_dylib(fixtures.base_str())?)
        .with_modules([host.clone()])?
        .relocate()?;
    let middle = RELOCATOR
        .run(loader.load_dylib(fixtures.middle_str())?)
        .with_modules([host.clone(), ModuleHandle::from(&base)])?
        .relocate()?;
    let leaf = RELOCATOR
        .run(loader.load_dylib(fixtures.leaf_str())?)
        .with_modules([host, ModuleHandle::from(&base), ModuleHandle::from(&middle)])?
        .relocate()?;
    let f = unsafe {
        base.get::<extern "C" fn() -> i32>("base_value")
            .expect("symbol lookup failed")
            .unwrap()
    };
    assert!(f() == 1);
    let f = unsafe {
        middle
            .get::<extern "C" fn() -> i32>("middle_value")
            .expect("symbol lookup failed")
            .unwrap()
    };
    assert!(f() == 2);
    let f = unsafe {
        leaf.get::<extern "C" fn() -> i32>("leaf_value")
            .expect("symbol lookup failed")
            .unwrap()
    };
    assert!(f() == 3);
    Ok(())
}
