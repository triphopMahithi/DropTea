use serde::Deserialize;
use std::fs;
use crate::core::engine::TransportMode;

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub storage: StorageConfig,
    #[serde(default)] 
    pub dev: Option<DevConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub port: u16,
    pub buffer_size: usize,
    #[serde(default = "default_mode")] 
    pub mode: String,
    
    // 🟢 UPDATED: รับค่า node_name จาก Config (Optional)
    pub node_name: Option<String>,
}

fn default_mode() -> String { "tcp".to_string() }

#[derive(Debug, Deserialize, Clone)]
pub struct StorageConfig {
    pub save_path: String,
    pub temp_path: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DevConfig {
    pub enabled: bool,
}

impl AppConfig {
    pub fn load_from_file(path: &str) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        let config: AppConfig = toml::from_str(&content)?;
        Ok(config)
    }

    // แปลง File Config เป็น Engine Config
    pub fn to_engine_config(&self) -> crate::core::engine::DropTeaConfig {
        let mode = match self.server.mode.to_lowercase().as_str() {
            "quic" => TransportMode::Quic,
            "plaintcp" | "plain_tcp" => TransportMode::PlainTcp,
            _ => TransportMode::Tcp,
        };
        
        crate::core::engine::DropTeaConfig {
            mode,
            port: self.server.port,
            storage_path: self.storage.save_path.clone(),
            // 🟢 UPDATED: ใช้ค่าจาก Config ถ้ามี ถ้าไม่มีให้ใช้ Device Name ของเครื่อง
            node_name: self.server.node_name.clone().unwrap_or_else(|| whoami::devicename()),
            dev_mode: self.dev.as_ref().map(|d| d.enabled).unwrap_or(false),
        }
    }
}