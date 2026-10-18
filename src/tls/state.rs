use super::{TlsModule, TlsRegistration, TlsTemplate};
use crate::{Result, TlsError, memory::MappedView};
use alloc::{boxed::Box, vec::Vec};
use spin::Mutex;

struct RegisteredTls {
    owner: TlsRegistration,
    image: &'static [u8],
}

/// TLS ownership and descriptor resources retained by the containing image.
pub(crate) struct ImageTlsState {
    descriptors: Mutex<Vec<Box<dyn Send + Sync>>>,
    registration: Option<RegisteredTls>,
}

impl ImageTlsState {
    pub(crate) fn empty() -> Self {
        Self {
            descriptors: Mutex::new(Vec::new()),
            registration: None,
        }
    }

    pub(crate) fn owned(owner: TlsRegistration, image: MappedView<u8>) -> Self {
        Self {
            descriptors: Mutex::new(Vec::new()),
            registration: Some(RegisteredTls {
                owner,
                image: image.as_slice(),
            }),
        }
    }

    pub(crate) fn module(&self) -> Option<&TlsModule> {
        self.registration.as_ref().map(|tls| tls.owner.module())
    }

    /// Copies only the initialized bytes, after relocation has finished.
    pub(crate) fn template(&self) -> Result<TlsTemplate> {
        self.registration
            .as_ref()
            .map(|tls| TlsTemplate::new(tls.image))
            .ok_or_else(|| TlsError::TemplateUnavailable.into())
    }

    pub(crate) fn retain_descriptor(&self, state: Option<Box<dyn Send + Sync>>) {
        if let Some(state) = state {
            self.descriptors.lock().push(state);
        }
    }

    /// Called after finalizers, while the mapping and runtime are still alive.
    pub(crate) fn release(&mut self) {
        self.descriptors.get_mut().clear();
        self.registration.take();
    }
}
