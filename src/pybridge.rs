#[cfg(feature = "python")]
use pyo3::prelude::*;
#[cfg(feature = "python")]
use std::sync::{Arc, Mutex};
#[cfg(feature = "python")]
use log::error;

#[cfg(feature = "python")]
use crate::core::transfer::{TransferCallback, CertificateAction};

#[cfg(feature = "python")]
#[derive(Clone)]
pub struct PyTransferCallback {
    callback: Arc<Mutex<PyObject>>,
}

#[cfg(feature = "python")]
impl PyTransferCallback {
    pub fn new(callback: PyObject) -> Self {
        Self {
            callback: Arc::new(Mutex::new(callback)),
        }
    }
}

#[cfg(feature = "python")]
impl TransferCallback for PyTransferCallback {
    // 🔥 แพ็ค String: Name|IP|Port|SSID|Transport
    fn on_peer_found(&self, id: &str, name: &str, ip: &str, port: u16, ssid: Option<&str>, transport: &str) {
        let cb = self.callback.lock().unwrap();
        let ssid_str = ssid.unwrap_or("");
        let data = format!("{}|{}|{}|{}|{}", name, ip, port, ssid_str, transport);
        Python::with_gil(|py| { let _ = cb.call1(py, ("PEER_FOUND", id, data)); });
    }

    fn on_peer_lost(&self, id: &str) {
        let cb = self.callback.lock().unwrap();
        Python::with_gil(|py| { let _ = cb.call1(py, ("PEER_LOST", id, "")); });
    }

    fn ask_accept_file(
        &self, 
        task_id: &str, 
        filename: &str, 
        filesize: u64,
        sender_name: &str,
        sender_device: &str
    ) -> anyhow::Result<bool> {
        let cb = self.callback.lock().unwrap();
        Python::with_gil(|py| {
            let result = cb.call1(
                py,
                ("ask_accept_file", task_id, filename, filesize, sender_name, sender_device),
            );
            match result {
                Ok(py_bool) => Ok(py_bool.extract::<bool>(py).unwrap_or(false)),
                Err(e) => { error!("Python callback error: {}", e); Ok(false) }
            }
        })
    }

    fn ask_verify_certificate(
        &self,
        peer_id: &str,
        fingerprint: &str,
        filename: Option<&str>,
    ) -> anyhow::Result<CertificateAction> {
        let cb = self.callback.lock().unwrap();
        Python::with_gil(|py| {
            let result = cb.call1(py, ("ask_verify_certificate", peer_id, fingerprint, filename));
            match result {
                Ok(val) => {
                    let accepted: bool = val.extract(py).unwrap_or(false);
                    if accepted { Ok(CertificateAction::Accept) } else { Ok(CertificateAction::Reject) }
                },
                Err(e) => { error!("Python cert verify error: {}", e); Ok(CertificateAction::Reject) }
            }
        })
    }

    fn on_start(&self, task_id: &str, filename: &str) {
        let cb = self.callback.lock().unwrap();
        Python::with_gil(|py| { let _ = cb.call1(py, ("START", task_id, filename)); });
    }

    fn on_progress(&self, task_id: &str, current: u64, total: u64) {
        let cb = self.callback.lock().unwrap();
        Python::with_gil(|py| { let _ = cb.call1(py, ("PROGRESS", task_id, (current, total))); });
    }

    fn on_complete(&self, task_id: &str, info: &str) {
        let cb = self.callback.lock().unwrap();
        Python::with_gil(|py| { let _ = cb.call1(py, ("COMPLETED", task_id, info)); });
    }

    fn on_error(&self, task_id: &str, error: &str) {
        let cb = self.callback.lock().unwrap();
        Python::with_gil(|py| { let _ = cb.call1(py, ("ERROR", task_id, error)); });
    }

    fn on_reject(&self, task_id: &str, reason: &str) {
        let cb = self.callback.lock().unwrap();
        Python::with_gil(|py| { let _ = cb.call1(py, ("REJECTED", task_id, reason)); });
    }
}