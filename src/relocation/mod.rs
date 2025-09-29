use crate::{CoreComponent, Error, relocate_error};
use alloc::{boxed::Box, format};
use core::any::Any;

pub(crate) mod dynamic_link;
pub(crate) mod static_link;

#[cold]
fn reloc_error(
    r_type: usize,
    r_sym: usize,
    custom_err: Box<dyn Any + Send + Sync>,
    lib: &CoreComponent,
) -> Error {
    if r_sym == 0 {
        relocate_error(
            format!(
                "file: {}, relocation type: {}, no symbol",
                lib.shortname(),
                r_type,
            ),
            custom_err,
        )
    } else {
        relocate_error(
            format!(
                "file: {}, relocation type: {}, symbol name: {}",
                lib.shortname(),
                r_type,
                lib.symtab().unwrap().symbol_idx(r_sym).1.name(),
            ),
            custom_err,
        )
    }
}
