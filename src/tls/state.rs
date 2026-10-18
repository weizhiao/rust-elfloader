use super::TlsModule;
use crate::{Result, TlsError, memory::MappedView};
use alloc::{boxed::Box, vec::Vec};
use spin::Mutex;

struct RegisteredTls {
    module: TlsModule,
    image: &'static [u8],
    owned: bool,
}

/// TLS metadata and descriptor resources. The containing image manages registration.
pub(crate) struct ImageTlsState {
    descriptors: Mutex<Vec<Box<dyn Send + Sync>>>,
    registration: Option<RegisteredTls>,
}

impl ImageTlsState {
    /// Borrows an existing registration; its caller retains ownership.
    pub(crate) fn borrowed(registration: Option<(TlsModule, MappedView<u8>)>) -> Self {
        Self {
            descriptors: Mutex::new(Vec::new()),
            registration: registration.map(|(module, image)| RegisteredTls {
                module,
                image: image.as_slice(),
                owned: false,
            }),
        }
    }

    /// Records a registration owned by the containing image.
    pub(crate) fn owned(module: TlsModule, image: MappedView<u8>) -> Self {
        Self {
            descriptors: Mutex::new(Vec::new()),
            registration: Some(RegisteredTls {
                module,
                image: image.as_slice(),
                owned: true,
            }),
        }
    }

    pub(crate) fn module(&self) -> Option<&TlsModule> {
        self.registration.as_ref().map(|tls| &tls.module)
    }

    /// Returns the registration that the containing image must unregister on drop.
    pub(crate) fn owned_module(&self) -> Option<&TlsModule> {
        self.registration
            .as_ref()
            .filter(|tls| tls.owned)
            .map(|tls| &tls.module)
    }

    pub(crate) fn image(&self) -> Result<&[u8]> {
        self.registration
            .as_ref()
            .map(|tls| tls.image)
            .ok_or_else(|| TlsError::TemplateUnavailable.into())
    }

    pub(crate) fn retain_descriptor(&self, state: Option<Box<dyn Send + Sync>>) {
        if let Some(state) = state {
            self.descriptors.lock().push(state);
        }
    }

    /// Releases descriptor arguments before their registration is removed.
    pub(crate) fn clear_descriptors(&mut self) {
        self.descriptors.get_mut().clear();
    }
}
