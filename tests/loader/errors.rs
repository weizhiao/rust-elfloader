use elf_loader::{Error, Loader};

#[test]
fn missing_path_is_io_error() {
    let error = Loader::new()
        .run(elf_loader::Domain::native(
            elf_loader::AddressSpaceId::PROCESS,
        ))
        .load_dylib("target/this_location_is_definitely_non existent:^~")
        .expect_err("loading a missing path should fail");
    assert!(matches!(error, Error::Io(_)));
}
