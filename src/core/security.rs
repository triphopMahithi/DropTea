use std::sync::{Arc, RwLock};
use std::collections::{HashMap, HashSet};
use std::time::SystemTime;
use std::fs; 
use std::path::{Path, PathBuf};
use rustls::{Certificate, PrivateKey, ServerName, ClientConfig, ServerConfig};
use rustls::client::{ServerCertVerified, ServerCertVerifier};
use rcgen::generate_simple_self_signed;
use blake3;
use anyhow::{Context, Result as AnyResult};
use log::{info, error, warn};
use serde::{Serialize, Deserialize};

use crate::core::transfer::{TransferCallback, CertificateAction};

// ==========================================
// 1. Data Structures for Storage
// ==========================================

#[derive(Serialize, Deserialize, Debug, Clone)]
struct KnownHostsStore {
    hosts: HashMap<String, String>, // IP/Hostname -> Fingerprint
}

impl Default for KnownHostsStore {
    fn default() -> Self {
        Self { hosts: HashMap::new() }
    }
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
struct WhitelistStore {
    trusted_senders: HashSet<String>,
}

// ==========================================
// 2. Security Manager (Thread-Safe State)
// ==========================================

pub struct SecurityManager {
    base_path: PathBuf,
    known_hosts: Arc<RwLock<KnownHostsStore>>,
    whitelist: Arc<RwLock<WhitelistStore>>,
}

impl SecurityManager {
    pub fn new(base_path: PathBuf) -> Arc<Self> {
        // Create directory if not exists
        let sec_path = base_path.join("security");
        if !sec_path.exists() {
            let _ = fs::create_dir_all(&sec_path);
        }

        // Load caches into memory
        let hosts = Self::load_known_hosts_from_disk(&sec_path);
        let whitelist = Self::load_whitelist_from_disk(&sec_path);

        Arc::new(Self {
            base_path: sec_path,
            known_hosts: Arc::new(RwLock::new(hosts)),
            whitelist: Arc::new(RwLock::new(whitelist)),
        })
    }

    // --- Internal Disk I/O Helpers ---

    fn load_known_hosts_from_disk(sec_path: &Path) -> KnownHostsStore {
        let path = sec_path.join("known_hosts.json");
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                match serde_json::from_str::<KnownHostsStore>(&content) {
                    Ok(store) => return store,
                    Err(e) => warn!("Failed to parse known_hosts.json: {}", e),
                }
            }
        }
        KnownHostsStore::default()
    }

    fn save_known_hosts_to_disk(&self, store: &KnownHostsStore) {
        let path = self.base_path.join("known_hosts.json");
        if let Ok(json) = serde_json::to_string_pretty(store) {
            if let Err(e) = fs::write(&path, json) {
                error!("Failed to write known_hosts.json: {}", e);
            }
        }
    }

    fn load_whitelist_from_disk(sec_path: &Path) -> WhitelistStore {
        let path = sec_path.join("whitelist.json");
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                return serde_json::from_str(&content).unwrap_or_default();
            }
        }
        WhitelistStore::default()
    }

    fn save_whitelist_to_disk(&self, store: &WhitelistStore) {
        let path = self.base_path.join("whitelist.json");
        if let Ok(json) = serde_json::to_string_pretty(store) {
            let _ = fs::write(path, json);
        }
    }

    // --- Public Logic (Thread-Safe) ---

    pub fn get_known_fingerprint(&self, peer_id: &str) -> Option<String> {
        let guard = self.known_hosts.read().unwrap();
        guard.hosts.get(peer_id).cloned()
    }

    pub fn save_known_host(&self, peer_id: String, fingerprint: String) {
        let mut guard = self.known_hosts.write().unwrap();
        
        // Double-check to optimize IO (if value is same, don't write disk)
        if let Some(existing) = guard.hosts.get(&peer_id) {
            if existing == &fingerprint {
                return;
            }
        }

        // ✅ FIXED: Clone key for insertion so we can use `peer_id` in log later
        guard.hosts.insert(peer_id.clone(), fingerprint);
        
        // Persist to disk under lock to prevent race condition on file write
        self.save_known_hosts_to_disk(&guard);
        info!("Updated known_host for {}", peer_id); 
    }

    pub fn is_trusted(&self, sender_name: &str) -> bool {
        let guard = self.whitelist.read().unwrap();
        guard.trusted_senders.contains(sender_name)
    }

    pub fn add_trust(&self, sender_name: String) {
        let mut guard = self.whitelist.write().unwrap();
        if !guard.trusted_senders.contains(&sender_name) {
            guard.trusted_senders.insert(sender_name);
            self.save_whitelist_to_disk(&guard);
        }
    }
}

// ==========================================
// 3. Helper Functions (Compatibility Layer)
// ==========================================

pub fn is_trusted(base_path: &str, sender_name: &str) -> bool {
    let path = PathBuf::from(base_path);
    let manager = SecurityManager::new(path);
    manager.is_trusted(sender_name)
}

pub fn add_trust(base_path: &str, sender_name: String) {
    let path = PathBuf::from(base_path);
    let manager = SecurityManager::new(path);
    manager.add_trust(sender_name);
}

// ==========================================
// 4. Identity Management
// ==========================================

pub fn load_or_generate_identity(storage_path: &str, node_name: &str) -> AnyResult<(Vec<Certificate>, PrivateKey)> {
    let base_path = PathBuf::from(storage_path);
    let sec_path = base_path.join("security");
    if !sec_path.exists() {
        fs::create_dir_all(&sec_path).context("Failed to create security directory")?;
    }
    
    let cert_path = sec_path.join(format!("{}_cert.der", node_name));
    let key_path = sec_path.join(format!("{}_key.der", node_name));

    if cert_path.exists() && key_path.exists() {
        info!("Loading persistent identity: {}", node_name);
        let cert_der = fs::read(&cert_path).context("Failed to read cert")?;
        let key_der = fs::read(&key_path).context("Failed to read key")?;
        return Ok((vec![Certificate(cert_der)], PrivateKey(key_der)));
    }

    info!("Generating NEW identity for: {}", node_name);
    let subject_alt_names = vec![node_name.to_string()];
    let cert = generate_simple_self_signed(subject_alt_names)?;
    let cert_der = cert.serialize_der()?;
    let key_der = cert.serialize_private_key_der();

    // Secure file writing
    {
        use std::io::Write;
        let mut f = fs::File::create(&key_path).context("Failed to create key file")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = f.metadata()?.permissions();
            perms.set_mode(0o600); // Read/Write only by owner
            f.set_permissions(perms)?;
        }
        f.write_all(&key_der).context("Failed to write key")?;
    }
    fs::write(&cert_path, &cert_der).context("Failed save cert")?;

    Ok((vec![Certificate(cert_der)], PrivateKey(key_der)))
}

pub fn generate_temp_identity() -> AnyResult<(Vec<Certificate>, PrivateKey)> {
    let subject_alt_names = vec!["droptea.temp".to_string()];
    let cert = generate_simple_self_signed(subject_alt_names)?;
    Ok((vec![Certificate(cert.serialize_der()?)], PrivateKey(cert.serialize_private_key_der())))
}

// ==========================================
// 5. TOFU Verifier (Updated to use Manager)
// ==========================================

pub struct TofuVerifier {
    manager: Arc<SecurityManager>, 
    callback: Option<Arc<dyn TransferCallback>>,
    filename: Option<String>,
}

impl TofuVerifier {
    pub fn new(manager: Arc<SecurityManager>) -> Arc<Self> {
        Arc::new(Self { manager, callback: None, filename: None })
    }

    pub fn with_callback(manager: Arc<SecurityManager>, callback: Arc<dyn TransferCallback>, filename: Option<String>) -> Arc<Self> {
        Arc::new(Self { manager, callback: Some(callback), filename })
    }

    fn check_cert(&self, cert: &Certificate, server_name: &ServerName) -> Result<(), rustls::Error> {
        let hash = blake3::hash(&cert.0);
        let fingerprint = hash.to_hex().to_string();
        
        let peer_id = match server_name {
            ServerName::DnsName(dns) => dns.as_ref().to_string(),
            ServerName::IpAddress(ip) => ip.to_string(),
            _ => "unknown".to_string(),
        };

        let clean_peer_id = peer_id.trim().to_string();
        
        // Use In-Memory Check (FAST)
        if let Some(known) = self.manager.get_known_fingerprint(&clean_peer_id) {
            if known == fingerprint {
                Ok(()) 
            } else {
                warn!("SECURITY ALERT: Fingerprint MISMATCH for {}", clean_peer_id);
                // MITM Protection / Key Rotation Check
                if let Some(cb) = &self.callback {
                    match cb.ask_verify_certificate(&clean_peer_id, &fingerprint, self.filename.as_deref()) {
                        Ok(CertificateAction::Accept) => {
                            info!("User ACCEPTED new fingerprint for {}. Updating...", clean_peer_id);
                            self.manager.save_known_host(clean_peer_id, fingerprint);
                            Ok(())
                        }
                        Ok(CertificateAction::Reject) => Err(rustls::Error::General("Certificate rejected by user".into())),
                        Err(e) => Err(rustls::Error::General(format!("Callback failed: {}", e)))
                    }
                } else {
                    Err(rustls::Error::General("Fingerprint mismatch (MITM protection)".into()))
                }
            }
        } else {
            // First Use (TOFU)
            if let Some(cb) = &self.callback {
                match cb.ask_verify_certificate(&clean_peer_id, &fingerprint, self.filename.as_deref()) {
                    Ok(CertificateAction::Accept) => {
                        self.manager.save_known_host(clean_peer_id, fingerprint);
                        Ok(())
                    }
                    Ok(CertificateAction::Reject) => Err(rustls::Error::General("Rejected by user".into())),
                    Err(e) => Err(rustls::Error::General(format!("Callback failed: {}", e)))
                }
            } else {
                // Silent Mode: Auto-trust first time
                self.manager.save_known_host(clean_peer_id, fingerprint);
                Ok(())
            }
        }
    }
}

impl ServerCertVerifier for TofuVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &Certificate,
        _intermediates: &[Certificate],
        server_name: &ServerName, 
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: SystemTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        self.check_cert(end_entity, server_name)?;
        Ok(ServerCertVerified::assertion())
    }
}

// ==========================================
// 6. TLS Config Builders
// ==========================================

pub fn build_tls_configs(storage_path: &str, node_name: &str) -> AnyResult<(ServerConfig, ClientConfig)> {
    let (certs, key) = load_or_generate_identity(storage_path, node_name)?; 
    
    // ✅ สร้าง Manager ตรงนี้
    let manager = SecurityManager::new(PathBuf::from(storage_path));
    let tofu = TofuVerifier::new(manager); 

    let server_config = ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(certs.clone(), key.clone())?;

    let client_config = ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(tofu)
        .with_client_auth_cert(certs, key)?;

    Ok((server_config, client_config))
}

pub fn build_temp_tls_configs() -> AnyResult<(ServerConfig, ClientConfig)> {
    let (certs, key) = generate_temp_identity()?;
    // ✅ สร้าง Temp Manager
    let manager = SecurityManager::new(Path::new("./downloads").to_path_buf());
    let tofu = TofuVerifier::new(manager);

    let server_config = ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(certs.clone(), key.clone())?;

    let client_config = ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(tofu)
        .with_client_auth_cert(certs, key)?;
        
    Ok((server_config, client_config))
}