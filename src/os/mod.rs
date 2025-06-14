cfg_if::cfg_if! {
    if #[cfg(windows)]{
        pub(crate) mod windows;
        pub use windows::*;
    }else if #[cfg(feature = "use-syscall")]{
        pub(crate) mod linux_syscall;
        pub use linux_syscall::*;
    }else if #[cfg(unix)]{
        pub(crate) mod unix;
        pub use unix::*;
    }else {
        pub(crate) mod bare;
        pub use bare::*;
    }
}
