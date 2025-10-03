use std::{env, process::Command};

fn main() {
    let ci = env::var("ELF_LOADER_CI").is_ok();
    if ci {
        println!("cargo:rerun-if-changed=always_trigger_rebuild");
        assert!(
            Command::new("bash")
                .arg("ci/prepare.sh")
                .status()
                .expect("failed to execute prepare.sh")
                .success()
        );
    }
}
