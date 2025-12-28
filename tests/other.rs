#[test]
fn wrong_name_fails() {
    let mut loader = elf_loader::Loader::new();
    let _ = loader
        .load_dylib("target/this_location_is_definitely_non existent:^~")
        .err()
        .unwrap();
}
