//! Data structures shared by lazy binders and the runtime resolver.

use crate::memory::VmAddr;
use alloc::boxed::Box;

/// Selects how an architecture installs lazy-binding runtime entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LazyPlacement {
    /// The context and resolver use fixed indexes relative to `DT_PLTGOT`.
    Slots {
        /// Slot index receiving the binder context.
        context: usize,
        /// Slot index receiving the resolver entry point.
        resolver: usize,
    },
    /// The architecture installs entries through its custom relocation handler.
    Custom,
}

/// Target-visible values supplied by a lazy binder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LazyValues {
    context: VmAddr,
    resolver: VmAddr,
}

impl LazyValues {
    /// Returns the binder context value.
    #[inline]
    pub const fn context(self) -> VmAddr {
        self.context
    }

    /// Returns the resolver entry point.
    #[inline]
    pub const fn resolver(self) -> VmAddr {
        self.resolver
    }
}

/// Values and retained state prepared for an image's lazy PLT runtime.
pub struct LazySetup {
    values: LazyValues,
    _state: Option<Box<dyn Send + Sync>>,
}

impl LazySetup {
    /// Creates a setup from target-visible context and resolver values.
    #[inline]
    pub const fn new(context: VmAddr, resolver: VmAddr) -> Self {
        Self {
            values: LazyValues { context, resolver },
            _state: None,
        }
    }

    /// Creates a setup whose context points at retained host-side state.
    pub fn with_state<T>(state: T, resolver: VmAddr) -> Self
    where
        T: Send + Sync + 'static,
    {
        let state = Box::new(state);
        let context = VmAddr::from_ptr(state.as_ref());
        let state = state as Box<dyn Send + Sync>;
        Self {
            values: LazyValues { context, resolver },
            _state: Some(state),
        }
    }

    #[inline]
    pub(crate) const fn values(&self) -> LazyValues {
        self.values
    }
}
