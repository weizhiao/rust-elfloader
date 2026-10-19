use crate::{
    Result, TlsError,
    arch::{NativeArch, get_thread_pointer},
    memory::VmAddr,
    os::{current_thread_id, get_thread_local_ptr, register_thread_destructor},
    relocation::RelocationArch,
    sync::{AtomicUsize, OnceCell, Ordering},
    tls::{
        ThreadDtv, TlsContext, TlsIndex, TlsInfo, TlsModule, TlsModuleId, TlsOwner, TlsProvider,
        TlsRegistration, TlsRegistry, TlsRequest, TlsResolver, TlsTemplate, TlsTpOffset,
    },
};
use alloc::{boxed::Box, collections::BTreeMap};
use core::{ffi::c_void, ptr::NonNull};
use spin::{Mutex, RwLock};

/// Global registry for all loaded modules' TLS metadata.
/// This allows any thread to look up how to initialize TLS for a specific module ID.
static MODULE_REGISTRY: RwLock<TlsRegistry> = RwLock::new(TlsRegistry::new());

#[inline]
fn with_registry<T>(f: impl FnOnce(&TlsRegistry) -> T) -> T {
    let registry = MODULE_REGISTRY.read();
    f(&registry)
}

#[inline]
fn with_registry_mut<T>(f: impl FnOnce(&mut TlsRegistry) -> T) -> T {
    let mut registry = MODULE_REGISTRY.write();
    let result = f(&mut registry);
    GLOBAL_GENERATION.store(registry.generation(), Ordering::Release);
    result
}

/// Global generation counter. Changes when registry membership changes.
/// DTVs use this to detect if they are stale and need updating.
static GLOBAL_GENERATION: AtomicUsize = AtomicUsize::new(0);

fn sync_dtv(dtv: &mut ThreadDtv) {
    let generation = GLOBAL_GENERATION.load(Ordering::Acquire);
    if dtv.generation() == generation {
        return;
    }
    let Some(snapshot) = with_registry(|registry| registry.snapshot_since(dtv.generation())) else {
        return;
    };
    dtv.sync(&snapshot);
}

// -----------------------------------------------------------------------------
// Thread Identity and Global Map
// -----------------------------------------------------------------------------

// We simulate TLS by mapping ThreadID -> ThreadDtv.
// This avoids touching thread registers directly.

type ThreadId = usize;

/// The global map of thread DTVs.
/// We use Box<ThreadDtv> to ensure the pointer remains stable even if the map rebalances/grows.
static THREAD_DTVS: Mutex<BTreeMap<ThreadId, Box<ThreadDtv>>> = Mutex::new(BTreeMap::new());

unsafe extern "C" fn dtv_destructor(_ptr: *mut c_void) {
    cleanup_current_thread_tls();
}

/// Accesses the current thread's DTV, optionally creating it on a cache miss.
/// A non-creating lookup does not register a destructor or allocate thread state.
fn with_current_dtv<R>(create: bool, f: impl FnOnce(&mut ThreadDtv) -> R) -> Option<R> {
    let cached = unsafe { get_thread_local_ptr() }.cast::<ThreadDtv>();
    let ptr = if cached.is_null() {
        let tid = current_thread_id();
        let mut map = THREAD_DTVS.lock();
        let dtv = if create {
            map.entry(tid).or_insert_with(|| Box::new(ThreadDtv::new()))
        } else {
            map.get_mut(&tid)?
        };
        let ptr = &mut **dtv as *mut ThreadDtv;
        drop(map);
        if create {
            // Also caches the stable pointer on backends with a native TLS slot.
            unsafe { register_thread_destructor(dtv_destructor, ptr.cast()) };
        }
        ptr
    } else {
        cached
    };
    // The box remains stable after releasing the map lock. Only this thread
    // accesses it; callers must end this borrow before image/provider callbacks.
    Some(unsafe { f(&mut *ptr) })
}

// -----------------------------------------------------------------------------
// Public APIs
// -----------------------------------------------------------------------------

/// TLS resolver for code running in the current process.
/// Clones share the global registry; each thread has its own TLS blocks.
#[derive(Clone, Copy, Debug)]
pub struct DefaultTlsResolver;

impl DefaultTlsResolver {
    /// Creates a default TLS resolver handle.
    pub const fn new() -> Self {
        Self
    }

    /// Get the current thread pointer.
    /// This uses architecture-specific methods to retrieve the thread pointer.
    pub fn get_thread_pointer() -> *mut u8 {
        unsafe { get_thread_pointer() }
    }

    /// Returns the registered TLS segment metadata for a module.
    pub fn get_info(mod_id: TlsModuleId) -> Option<TlsInfo> {
        with_registry(|registry| registry.info(mod_id))
    }

    /// Registers a custom provider in the default runtime's namespace.
    /// The returned owner unregisters and releases the provider on drop.
    ///
    /// # Safety
    /// `info` must describe the provider's TLS block. If supplied, `tp_offset`
    /// must identify that block in every current and future thread. A pointer
    /// observed in one thread alone is not proof of static placement. The
    /// provider must support this runtime's namespace and thread storage.
    pub unsafe fn register_provider<P: TlsProvider>(
        &self,
        info: TlsInfo,
        tp_offset: Option<TlsTpOffset>,
        provider: P,
    ) -> Result<TlsRegistration> {
        let entry = self.bind_tls_get_addr()?;
        let owner = TlsOwner::new(provider);
        let module =
            with_registry_mut(|registry| unsafe { registry.register(info, tp_offset, &owner) })?;
        let mod_id = module.mod_id();
        // SAFETY: This guard is the only owner of the newly inserted entry.
        Ok(unsafe {
            TlsRegistration::new(TlsModule::new(module, entry), move || {
                Self::new().unregister(mod_id);
            })
        })
    }

    /// Resolves the current thread's block, allocating managed or host TLS if needed.
    /// The raw pointer must not be used from another thread or after module/thread
    /// teardown. No reference or aliasing guarantee is implied.
    pub fn get_ptr(mod_id: TlsModuleId) -> Option<*mut u8> {
        resolve(mod_id, 0).ok().map(NonNull::as_ptr)
    }

    /// Queries an existing block without allocating TLS or creating a DTV.
    pub fn get_ptr_if_allocated(mod_id: TlsModuleId) -> Option<*mut u8> {
        let entry = with_registry(|registry| registry.lookup(mod_id)).ok()?;
        entry.get().map(NonNull::as_ptr)
    }
}

impl Default for DefaultTlsResolver {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl TlsResolver<NativeArch> for DefaultTlsResolver {
    const OVERRIDE_TLS_GET_ADDR: bool = true;

    fn register(&self, info: TlsInfo, request: TlsRequest) -> Result<TlsRegistration> {
        let tp_offset = match request {
            TlsRequest::Dynamic => None,
            TlsRequest::Static(Some(offset)) => Some(offset),
            TlsRequest::Static(None) => return Err(TlsError::ResolverUnsupported.into()),
        };
        // SAFETY: ManagedTls uses this runtime's thread cache and placement.
        unsafe { self.register_provider(info, tp_offset, ManagedTls::new()) }
    }

    fn publish(&self, source: TlsTemplate, mod_id: TlsModuleId) -> Result<()> {
        let entry = with_registry(|registry| registry.lookup(mod_id))?;
        entry.publish(source)
    }

    fn unregister(&self, mod_id: TlsModuleId) {
        let owner = with_registry_mut(|registry| registry.unregister(mod_id));
        // A provider's destructor can call the host loader. Never drop it under a lock.
        drop(owner);
    }

    fn bind_tls_get_addr(&self) -> Result<VmAddr> {
        Ok(VmAddr::from_ptr(tls_get_addr as *const ()))
    }
}

fn resolve(mod_id: TlsModuleId, offset: usize) -> Result<NonNull<u8>> {
    let entry = with_registry(|registry| registry.lookup(mod_id))?;
    entry.resolve(offset)
}

fn static_address(offset: TlsTpOffset) -> Option<NonNull<u8>> {
    let tp = unsafe { get_thread_pointer() };
    NonNull::new(tp.wrapping_offset(offset.get()))
}

/// Managed storage tied to the default runtime's registry and thread cache.
/// Custom registries must use a provider tied to their own thread state.
pub(super) struct ManagedTls {
    image: OnceCell<TlsTemplate>,
}

impl ManagedTls {
    pub(super) const fn new() -> Self {
        Self {
            image: OnceCell::new(),
        }
    }
}

unsafe impl TlsProvider for ManagedTls {
    unsafe fn publish(&self, _context: &TlsContext, source: TlsTemplate) -> Result<()> {
        assert!(
            self.image.set(source).is_ok(),
            "TLS template must be published only once",
        );
        Ok(())
    }

    unsafe fn resolve(&self, context: &TlsContext, offset: usize) -> Result<NonNull<u8>> {
        let source = self.image.get().ok_or(TlsError::TemplateUnavailable)?;
        let binding = context.binding();
        let base = if let Some(tp_offset) = binding.tp_offset() {
            static_address(tp_offset).ok_or(TlsError::InvalidInfo)?
        } else if let Some(base) = get_dynamic(binding.mod_id()) {
            base
        } else {
            with_current_dtv(true, |dtv| {
                sync_dtv(dtv);
                if !with_registry(|registry| registry.contains(binding.mod_id())) {
                    return Err(TlsError::InvalidModuleId {
                        mod_id: binding.mod_id(),
                    }
                    .into());
                }
                dtv.get_or_init(binding.mod_id(), context.info(), source.as_ref())
            })
            .expect("creating the current thread's DTV always returns a DTV")?
        };
        NonNull::new(base.as_ptr().wrapping_add(offset)).ok_or_else(|| TlsError::InvalidInfo.into())
    }

    unsafe fn get(&self, context: &TlsContext) -> Option<NonNull<u8>> {
        self.image.get()?;
        let binding = context.binding();
        if let Some(tp_offset) = binding.tp_offset() {
            static_address(tp_offset)
        } else {
            get_dynamic(binding.mod_id())
        }
    }
}

fn get_dynamic(mod_id: TlsModuleId) -> Option<NonNull<u8>> {
    with_current_dtv(false, |dtv| {
        sync_dtv(dtv);
        dtv.get(mod_id)
    })
    .flatten()
}

// Each entry uses the ABI of loaded ELF code, including on Windows.
#[cfg(all(windows, target_arch = "x86_64"))]
extern "sysv64" fn tls_get_addr(index: *const TlsIndex) -> *mut u8 {
    resolve_index(unsafe { &*index })
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
extern "C" fn tls_get_addr(index: *const TlsIndex) -> *mut u8 {
    resolve_index(unsafe { &*index })
}

fn resolve_index(ti: &TlsIndex) -> *mut u8 {
    match resolve(
        ti.ti_module,
        ti.ti_offset.wrapping_add(NativeArch::TLS_DTV_OFFSET),
    ) {
        Ok(address) => address.as_ptr(),
        Err(error) => panic!(
            "__tls_get_addr: failed to resolve TLS module {}: {error}",
            ti.ti_module
        ),
    }
}

/// Optional: Manually cleanup TLS for the current thread.
/// Should be called when a thread exits to prevent memory leaks in our map.
fn cleanup_current_thread_tls() {
    let tid = current_thread_id();
    let mut map = THREAD_DTVS.lock();
    map.remove(&tid);
}
