#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

pub fn get_path(name: &str) -> PathBuf {
    PathBuf::from(env!("TEST_ARTIFACTS")).join(name)
}

pub fn print_fn(s: &str) {
    println!("{}", s);
}

pub fn get_symbol_map() -> Arc<HashMap<&'static str, usize>> {
    let mut map = HashMap::new();
    map.insert("print", print_fn as *const () as usize);
    Arc::new(map)
}

pub fn get_pre_find() -> Arc<impl Fn(&str) -> Option<*const ()> + Send + Sync + 'static> {
    let map = get_symbol_map();
    Arc::new(move |name: &str| -> Option<*const ()> {
        map.get(name).copied().map(|p| p as *const ())
    })
}
