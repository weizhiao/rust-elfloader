mod common;

use common::{get_path, get_pre_find};
use elf_loader::{Relocatable, load_dylib};
use rstest::rstest;

#[rstest]
fn relocate_dylib() {
    let pre_find = get_pre_find();
    let liba = load_dylib!(get_path("liba.so").to_str().unwrap()).unwrap();
    let libb = load_dylib!(get_path("libb.so").to_str().unwrap()).unwrap();
    let libc = load_dylib!(get_path("libc.so").to_str().unwrap()).unwrap();

    let a = liba.relocator().symbols(&*pre_find).relocate().unwrap();
    let f = unsafe { *a.get::<fn() -> i32>("a").unwrap() };
    assert_eq!(f(), 1);

    let b = libb
        .relocator()
        .symbols(&*pre_find)
        .scope([&a].into_iter())
        .relocate()
        .unwrap();
    let f = unsafe { *b.get::<fn() -> i32>("b").unwrap() };
    assert_eq!(f(), 2);

    let c = libc
        .relocator()
        .symbols(&*pre_find)
        .scope([&b].into_iter())
        .relocate()
        .unwrap();
    let f = unsafe { *c.get::<fn() -> i32>("c").unwrap() };
    assert_eq!(f(), 3);
}

#[rstest]
fn lazy_binding() {
    let pre_find = get_pre_find();
    let liba = load_dylib!(get_path("liba.so").to_str().unwrap()).unwrap();
    let libb = load_dylib!(get_path("libb.so").to_str().unwrap()).unwrap();

    let a = liba.relocator().symbols(&*pre_find).relocate().unwrap();

    let pre_find_clone = pre_find.clone();
    let b = libb
        .relocator()
        .symbols(pre_find.clone())
        .scope([&a])
        .lazy(true)
        .lazy_scope(Some(pre_find_clone))
        .use_scope_as_lazy()
        .relocate()
        .unwrap();
    let f = unsafe { *b.get::<fn() -> i32>("b").unwrap() };
    assert_eq!(f(), 2);
}

// This test uses C shared libraries compiled by build.rs:
// libfoo.so exports `int foo(void)` and libbar.so calls `foo()`.
// Linking libbar against libfoo will create PLT/JUMP_SLOT relocations
// which the loader should handle (both eager and lazy binding paths).
#[rstest]
#[cfg(unix)]
fn generate_and_test_plt_relocations() {
    let foo_so = get_path("libfoo.so");
    let bar_so = get_path("libbar.so");

    if !foo_so.exists() || !bar_so.exists() {
        eprintln!("Skipping test: libfoo.so or libbar.so not found (C compiler missing?)");
        return;
    }

    let pre_find = get_pre_find();

    let libfoo = load_dylib!(foo_so.to_str().unwrap()).unwrap();
    let libbar = load_dylib!(bar_so.to_str().unwrap()).unwrap();

    // First, relocate libfoo normally
    let foo = libfoo
        .relocator()
        .symbols(&*pre_find)
        .lazy(false)
        .relocate()
        .unwrap();

    // Relocate libbar eagerly (non-lazy) to exercise immediate PLT resolution path
    let bar = libbar
        .relocator()
        .symbols(&*pre_find)
        .scope([&foo].into_iter())
        .lazy(false)
        .relocate()
        .unwrap();

    let f: extern "C" fn() -> i32 = unsafe { *bar.get::<extern "C" fn() -> i32>("bar").unwrap() };
    assert_eq!(f(), 2);

    // Also test lazy binding path: reload libbar with lazy=true
    let libbar = load_dylib!(bar_so.to_str().unwrap()).unwrap();
    let _pre_find_clone = pre_find.clone();
    let bar_lazy = libbar
        .relocator()
        .symbols(pre_find.clone())
        .scope([&foo].into_iter())
        .lazy(true)
        .use_scope_as_lazy()
        .relocate()
        .unwrap();
    let f_lazy: extern "C" fn() -> i32 =
        unsafe { *bar_lazy.get::<extern "C" fn() -> i32>("bar").unwrap() };
    assert_eq!(f_lazy(), 2);
}
