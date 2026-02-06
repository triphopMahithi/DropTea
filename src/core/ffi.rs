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
        // ป้องกัน Null Byte Injection
        let to_c = |s: &str| CString::new(s.replace("\0", "")).unwrap_or_default();
        let empty = CString::new("").unwrap();
        
        match event {
            TransferEvent::Log { msg, .. } => {
                (self.callback)(0, empty.as_ptr(), to_c(&msg).as_ptr(), empty.as_ptr(), 0, 0)
            },
            TransferEvent::ServerStarted { port } => {
                let p_str = port.to_string();
                (self.callback)(10, empty.as_ptr(), to_c(&p_str).as_ptr(), empty.as_ptr(), 0, 0)
            },
            TransferEvent::PeerFound { id, name, ip, port, ssid, transport } => {
               let data = format!("{}|{}|{}|{}|{}", name, ip, port, ssid.unwrap_or_default(), transport);
               (self.callback)(1, to_c(&id).as_ptr(), to_c(&data).as_ptr(), empty.as_ptr(), 0, 0)
            },
            TransferEvent::Progress { task_id, current, total } => {
                (self.callback)(3, to_c(&task_id).as_ptr(), empty.as_ptr(), empty.as_ptr(), current, total)
            },
            TransferEvent::Completed { task_id, info } => {
                 (self.callback)(4, to_c(&task_id).as_ptr(), to_c(&info).as_ptr(), empty.as_ptr(), 0, 0)
            },
            TransferEvent::Incoming { task_id, filename } => {
                 (self.callback)(6, to_c(&task_id).as_ptr(), to_c(&filename).as_ptr(), empty.as_ptr(), 0, 0)
            },
            TransferEvent::Error { task_id, error } => {
                 (self.callback)(5, to_c(&task_id).as_ptr(), to_c(&error).as_ptr(), empty.as_ptr(), 0, 0)
            },
            TransferEvent::Started { task_id, msg } => {
                (self.callback)(2, to_c(&task_id).as_ptr(), to_c(&msg).as_ptr(), empty.as_ptr(), 0, 0)
            },
            TransferEvent::Rejected { task_id, reason } => {
                (self.callback)(7, to_c(&task_id).as_ptr(), to_c(&reason).as_ptr(), empty.as_ptr(), 0, 0)
            },
            TransferEvent::PeerLost { id } => {
                (self.callback)(8, to_c(&id).as_ptr(), empty.as_ptr(), empty.as_ptr(), 0, 0)
            },
            TransferEvent::DiscoveryStarted => {
                (self.callback)(9, empty.as_ptr(), empty.as_ptr(), empty.as_ptr(), 0, 0)
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn droptea_init(storage_path: *const c_char, port: u16, mode: c_int, callback: CppCallback) -> *mut c_void {
    let c_str = unsafe { CStr::from_ptr(storage_path) };
    let path_str = c_str.to_string_lossy().into_owned();
    let rt = Arc::new(Runtime::new().unwrap());
    let handler = Box::new(CppEventHandlerAdapter { callback });

    // 🟢 UPDATED: Mapping Mode 2 -> PlainTcp
    let transport_mode = match mode {
        1 => TransportMode::Quic,
        2 => TransportMode::PlainTcp,
        _ => TransportMode::Tcp,
    };

    let config = DropTeaConfig {
        mode: transport_mode,
        port: port,
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