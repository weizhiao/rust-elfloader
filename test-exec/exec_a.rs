use std::process::Command;
use std::thread;
use std::time::Duration;

fn main() {
    Command::new("sh")
        .arg("-c")
        .arg("ps -ef | tail")
        .output()
        .expect("Failed to execute command");

    thread::sleep(Duration::from_secs(2));
}
