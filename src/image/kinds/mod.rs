mod dylib;
mod exec;
mod object;

pub(crate) use exec::StaticImage;

pub use dylib::{RawDylib, LoadedDylib};
pub use exec::{RawExec, LoadedExec};
pub use object::{LoadedObject, RawObject};
