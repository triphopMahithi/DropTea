pub mod core;

#[cfg(feature = "python")]
pub mod python_api {
    use super::*;
    use pyo3::prelude::*;
    use std::sync::{Arc, RwLock};
    use tokio::runtime::Runtime;
    
    use crate::core::engine::{DropTeaCore, DropTeaConfig, TransportMode};
    use crate::core::events::TransferEvent; 
    use crate::core::events::TransferEventHandler;
    use crate::core::utils;
    use crate::core::handshake;
    use crate::core::config::AppConfig; 

    struct PyEventHandler {
        callback: PyObject,
        rt: tokio::runtime::Handle,
    }
    
    impl TransferEventHandler for PyEventHandler {
        fn on_event(&self, event: TransferEvent) {
            let callback = self.callback.clone();
            let (evt_type, arg1, arg2) = match event {
                TransferEvent::Log { msg, .. } => ("LOG".to_string(), msg, "".to_string()),
                TransferEvent::ServerStarted { port } => ("SERVER_STARTED".to_string(), port.to_string(), "".to_string()),
                TransferEvent::Error { task_id, error } => ("ERROR".to_string(), task_id, error),
                TransferEvent::Incoming { task_id, filename } => ("Incoming".to_string(), task_id, filename),
                TransferEvent::Started { task_id, msg } => ("START".to_string(), task_id, msg),
                TransferEvent::Progress { task_id, current, total } => ("PROGRESS".to_string(), task_id, format!("{}|{}", current, total)),
                TransferEvent::Completed { task_id, info } => ("COMPLETED".to_string(), task_id, info),
                TransferEvent::Rejected { task_id, reason } => ("REJECTED".to_string(), task_id, reason),
                TransferEvent::DiscoveryStarted => ("DISCOVERY_STARTED".to_string(), "".to_string(), "".to_string()),
                TransferEvent::PeerFound { id, name, ip, port, ssid, transport } => {
                    let data = format!("{}|{}|{}|{}|{}", name, ip, port, ssid.unwrap_or_default(), transport);
                    ("PEER_FOUND".to_string(), id, data)
                },
                TransferEvent::PeerLost { id } => ("PEER_LOST".to_string(), id, "".to_string()),
            };
            self.rt.spawn(async move {
                Python::with_gil(|py| { 
                    if let Err(e) = callback.call1(py, (evt_type, arg1, arg2)) { 
                        e.print(py); 
                    } 
                });
            });
        }
    }

    #[pyclass]
    struct DropTeaEngine {
        core: Arc<RwLock<Arc<DropTeaCore>>>,
        rt: Arc<Runtime>,
    }

    #[pymethods]
    impl DropTeaEngine {
        #[new]
        fn new() -> PyResult<Self> {
            let rt = Arc::new(Runtime::new().unwrap());
            struct NoOp; impl TransferEventHandler for NoOp { fn on_event(&self, _: TransferEvent) {} }
            let config = DropTeaConfig {
                mode: TransportMode::Tcp,
                port: 0,
                storage_path: ".".to_string(),
                node_name: "init".to_string(),
                dev_mode: false,
            };
            let core = DropTeaCore::new_with_config(rt.clone(), config, Box::new(NoOp))
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
            Ok(DropTeaEngine { core: Arc::new(RwLock::new(Arc::new(core))), rt })
        }

        fn get_my_name(&self) -> String { utils::get_system_name() }

        fn start_server(&self, config_path: String, callback: PyObject) -> PyResult<()> {
            let py_handler = PyEventHandler { callback, rt: self.rt.handle().clone() };
            let app_config = AppConfig::load_from_file(&config_path)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("Config Load Failed: {}", e)))?;
            let engine_config = app_config.to_engine_config();
            let port = engine_config.port;
            let real_core = DropTeaCore::new_with_config(self.rt.clone(), engine_config, Box::new(py_handler))
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
            real_core.start_service(port);
            *self.core.write().unwrap() = Arc::new(real_core);
            Ok(())
        }
        
        #[pyo3(signature = (ip, port, file_path, task_id, callback, my_device_name=None, target_os=None))]
        fn send_file(&self, ip: String, port: u16, file_path: String, task_id: String, callback: PyObject, my_device_name: Option<String>, target_os: Option<String>) -> PyResult<()> {
            let core_guard = self.core.read().unwrap();
            let task_handler = PyEventHandler { callback, rt: self.rt.handle().clone() };
            core_guard.send_file(
                ip, port, file_path, task_id, 
                my_device_name.unwrap_or_else(|| utils::get_system_name()), 
                Box::new(task_handler),
                target_os
            );
            Ok(())
        }

        fn resolve_request(&self, task_id: String, accept: bool) -> PyResult<()> {
            self.core.read().unwrap().resolve_request(task_id, accept);
            Ok(())
        }
    } 

    #[pyfunction]
    fn send_handshake(py: Python, mac: String) -> PyResult<&PyAny> {
        pyo3_asyncio::tokio::future_into_py(py, async move {
            match handshake::connect_and_say_hello(mac).await { 
                Ok(_) => Ok(()), 
                Err(e) => Err(pyo3::exceptions::PyRuntimeError::new_err(format!("{}", e))), 
            }
        })
    }

    #[pyfunction]
    fn calculate_quick_hash(_py: Python, f: String, l: Option<u64>) -> PyResult<String> {
        utils::calculate_quick_hash(f, l)
            .map(|v| hex::encode(v))
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))
    }

    #[pyfunction] 
    fn compress_folder(_py: Python, f: String, z: String) -> PyResult<bool> { 
        utils::compress_folder(f, z).map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string())) 
    }

    #[pyfunction] 
    fn extract_zip(_py: Python, z: String, e: String) -> PyResult<bool> { 
        utils::extract_zip(z, e).map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string())) 
    }

    #[pyfunction] 
    fn preallocate_file(_py: Python, p: String, s: u64) -> PyResult<bool> { 
        utils::preallocate_file(p, s).map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string())) 
    }

    #[pymodule]
    fn droptea_core(_py: Python, m: &PyModule) -> PyResult<()> {
        pyo3_log::init();
        m.add_class::<DropTeaEngine>()?;
        m.add_function(wrap_pyfunction!(calculate_quick_hash, m)?)?;
        m.add_function(wrap_pyfunction!(compress_folder, m)?)?;
        m.add_function(wrap_pyfunction!(extract_zip, m)?)?;
        m.add_function(wrap_pyfunction!(preallocate_file, m)?)?;
        m.add_function(wrap_pyfunction!(send_handshake, m)?)?;
        Ok(())
    }
}

#[cfg(feature = "ffi")]
pub use core::ffi::*;