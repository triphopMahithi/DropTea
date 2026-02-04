use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::sync::{Arc, RwLock};
use tokio::runtime::Runtime;

use crate::core::engine::{DropTeaCore, DropTeaConfig, TransportMode};
use crate::core::events::{TransferEvent, TransferEventHandler};

type CppCallback = extern "C" fn(c_int, *const c_char, *const c_char, *const c_char, u64, u64);

pub struct DropTeaContext {
    core: RwLock<Arc<DropTeaCore>>,
    _rt: Arc<Runtime>, 
}

struct CppEventHandlerAdapter { callback: CppCallback }
impl TransferEventHandler for CppEventHandlerAdapter {
    fn on_event(&self, event: TransferEvent) {
        let to_c = |s: &str| CString::new(s).unwrap_or_default();
        let empty = CString::new("").unwrap();
        match event {
            TransferEvent::Log { msg, .. } => (self.callback)(0, empty.as_ptr(), to_c(&msg).as_ptr(), empty.as_ptr(), 0, 0),
            // ... (Mapping event อื่นๆ ถ้ามี) ...
            _ => {}
        }
    }
}

#[no_mangle]
pub extern "C" fn droptea_init(storage_path: *const c_char, mode: c_int, callback: CppCallback) -> *mut c_void {
    let c_str = unsafe { CStr::from_ptr(storage_path) };
    let path_str = c_str.to_string_lossy().into_owned();
    let rt = Arc::new(Runtime::new().unwrap());
    let handler = Box::new(CppEventHandlerAdapter { callback });

    let transport_mode = if mode == 1 { TransportMode::Quic } else { TransportMode::Tcp };

    let config = DropTeaConfig {
        mode: transport_mode,
        port: 0,
        storage_path: path_str,
        node_name: "ffi_node".to_string(),
        dev_mode: false, 
    };

    match DropTeaCore::new_with_config(rt.clone(), config, handler) {
        Ok(core) => {
            let context = Box::new(DropTeaContext {
                core: RwLock::new(Arc::new(core)),
                _rt: rt,
            });
            Box::into_raw(context) as *mut c_void
        }
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn droptea_start_service(ctx_ptr: *mut c_void, port: u16, device_id: *const c_char, dev_mode: bool) {
    if ctx_ptr.is_null() { return; }
    let context = &*(ctx_ptr as *mut DropTeaContext);
    context.core.read().unwrap().start_service(port);
}

#[no_mangle]
pub unsafe extern "C" fn droptea_resolve_request(ctx_ptr: *mut c_void, task_id: *const c_char, accept: bool) {
    if ctx_ptr.is_null() { return; }
    let context = &*(ctx_ptr as *mut DropTeaContext);
    let tid_s = CStr::from_ptr(task_id).to_string_lossy().into_owned();
    context.core.read().unwrap().resolve_request(tid_s, accept);
}

#[no_mangle]
pub unsafe extern "C" fn droptea_free(ctx_ptr: *mut c_void) {
    if !ctx_ptr.is_null() { let _ = Box::from_raw(ctx_ptr as *mut DropTeaContext); }
}