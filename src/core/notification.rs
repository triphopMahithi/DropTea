use tokio::sync::mpsc;
use std::sync::Mutex;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum WinToastError {
    NoError = 0,
    UnknownError = 8, 
}

impl From<i32> for WinToastError {
    fn from(_: i32) -> Self { WinToastError::UnknownError }
}

#[derive(Debug, Clone, PartialEq)]
pub enum UserResponse {
    Accept,
    Decline,
    Dismissed,
    Error(WinToastError),
}

#[cfg(target_os = "windows")]
mod backend {
    use super::*;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "wintoast_bridge")]
    extern "C" {
        fn init_wintoast(app_name: *const u16, aumid: *const u16) -> bool;
        fn show_request_toast(title: *const u16, msg: *const u16, img: *const u16, cb: extern "C" fn(i32));
        fn show_info_toast(title: *const u16, msg: *const u16, img: *const u16); // ✅ Bind function นี้
        fn create_shortcut_native(target: *const u16, args: *const u16, dir: *const u16, aumid: *const u16, name: *const u16) -> bool;
    }

    static SENDER: Mutex<Option<mpsc::UnboundedSender<UserResponse>>> = Mutex::new(None);

    extern "C" fn ffi_callback(code: i32) {
        let response = match code {
            0 => UserResponse::Accept,
            1 => UserResponse::Decline,
            -1 => UserResponse::Dismissed,
            _ => UserResponse::Error(WinToastError::UnknownError),
        };
        if let Ok(guard) = SENDER.lock() {
            if let Some(tx) = &*guard { let _ = tx.send(response); }
        }
    }

    fn to_wstring(str: &str) -> Vec<u16> {
        OsStr::new(str).encode_wide().chain(Some(0).into_iter()).collect()
    }

    pub fn setup_shortcut(python_exe: &str, script_path: &str) {
        let target = to_wstring(python_exe);
        let args = to_wstring(&format!("\"{}\"", script_path));
        
        let script_p = Path::new(script_path);
        let parent_dir = script_p.parent().unwrap_or(Path::new("."));
        let dir = to_wstring(parent_dir.to_str().unwrap());
        
        let aumid = to_wstring("DropTea"); 
        let name = to_wstring("DropTea");

        unsafe {
            create_shortcut_native(target.as_ptr(), args.as_ptr(), dir.as_ptr(), aumid.as_ptr(), name.as_ptr());
        }
    }

    pub fn init_system() -> bool {
        let app = to_wstring("DropTea");
        let aumid = to_wstring("DropTea"); 
        unsafe { init_wintoast(app.as_ptr(), aumid.as_ptr()) }
    }

    pub fn show_notification(title: &str, msg: &str, image_path: &str, tx: mpsc::UnboundedSender<UserResponse>) {
        if let Ok(mut guard) = SENDER.lock() { *guard = Some(tx); }
        let t = to_wstring(title);
        let m = to_wstring(msg);
        let i = to_wstring(image_path);
        tokio::task::spawn_blocking(move || { unsafe { show_request_toast(t.as_ptr(), m.as_ptr(), i.as_ptr(), ffi_callback); } });
    }

    // ✅ เพิ่มฟังก์ชันนี้เพื่อแก้ Error "not found"
    pub fn show_info(title: &str, msg: &str) {
        let t = to_wstring(title);
        let m = to_wstring(msg);
        let i = to_wstring(""); 
        tokio::task::spawn_blocking(move || { unsafe { show_info_toast(t.as_ptr(), m.as_ptr(), i.as_ptr()); } });
    }
}

#[cfg(not(target_os = "windows"))]
mod backend {
    use super::*;
    pub fn init_system() -> bool { true }
    pub fn setup_shortcut(_: &str, _: &str) {} 
    pub fn show_notification(_t: &str, _m: &str, _i: &str, tx: mpsc::UnboundedSender<UserResponse>) { let _ = tx.send(UserResponse::Accept); }
    pub fn show_info(_: &str, _: &str) {}
}

pub use backend::{init_system, show_notification, show_info, setup_shortcut};