use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Instant, Duration};
use std::net::{UdpSocket, IpAddr}; // 🟢 UPDATED: เพิ่ม IpAddr
use log::{info, error, debug, warn};
use mdns_sd::{ServiceDaemon, ServiceInfo, ServiceEvent};
use tokio::time::timeout;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use anyhow::Context;

// 📦 Dependencies
use futures::stream::StreamExt;
use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral, ScanFilter};
use btleplug::platform::Manager;
use dashmap::DashMap; 
use rand::Rng;       

use crate::core::transfer::TransferCallback;
use crate::core::utils;

// ==========================================
// 🎯 CONFIGURATION
// ==========================================
const DROPTEA_UUID_PART: &str = "d7ea";
const DROPTEA_NAME_PREFIX: &str = "DT-";
const HEALTH_CHECK_INTERVAL_SEC: u64 = 1; 
const PEER_STALE_THRESHOLD_SEC: u64 = 15; 
const BLE_CACHE_TTL_MS: u128 = 1000;      

// ==========================================
// 1. Data Structures
// ==========================================

#[derive(Clone, Debug, PartialEq)]
pub enum TransportType {
    Lan,
    BleOnly,
    Hybrid,
}

impl ToString for TransportType {
    fn to_string(&self) -> String {
        match self {
            TransportType::Lan => "LAN".to_string(),
            TransportType::BleOnly => "BLE".to_string(),
            TransportType::Hybrid => "HYBRID".to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct PeerInfo {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub ip: Option<IpAddr>, // 🟢 UPDATED: เปลี่ยนจาก String เป็น IpAddr (Strong Type)
    pub port: u16,
    pub ssid: Option<String>,
    pub ble_mac: Option<String>,
    pub transport: TransportType,
    pub last_seen: Instant,
    pub missed_pings: u32,
}

pub enum DiscoveryInternalEvent {
    MdnsFound { id: String, name: String, ip: String, port: u16 },
    MdnsLost { id: String },
    BleFound { id: String, name: String, ssid: Option<String>, mac: String },
}

// ==========================================
// 2. Discovery Engine
// ==========================================

#[derive(Clone)]
pub struct DiscoveryEngine<CB: TransferCallback> {
    pub daemon: ServiceDaemon,
    pub callback: CB,
    pub known_peers: Arc<DashMap<String, PeerInfo>>,
    event_tx: mpsc::Sender<DiscoveryInternalEvent>,
}

impl<CB: TransferCallback + Clone + Send + Sync + 'static> DiscoveryEngine<CB> {

    pub fn new(callback: CB) -> anyhow::Result<(Self, mpsc::Receiver<DiscoveryInternalEvent>)> {
        let daemon = ServiceDaemon::new()
            .map_err(|e| anyhow::anyhow!("Failed to create mDNS daemon: {}", e))?;

        let (tx, rx) = mpsc::channel(100);

        Ok((Self {
            daemon,
            callback,
            known_peers: Arc::new(DashMap::new()), 
            event_tx: tx,
        }, rx))
    }

    fn get_local_ip() -> String {
        match UdpSocket::bind("0.0.0.0:0") {
            Ok(s) => match s.connect("8.8.8.8:80") {
                Ok(_) => s.local_addr().map(|a| a.ip().to_string()).unwrap_or_else(|_| "127.0.0.1".to_string()),
                Err(_) => "127.0.0.1".to_string(),
            },
            Err(_) => "127.0.0.1".to_string(),
        }
    }

    pub async fn run_health_check(&self) {
        loop {
            tokio::time::sleep(Duration::from_secs(HEALTH_CHECK_INTERVAL_SEC)).await;

            let suspects: Vec<(String, IpAddr, u16, String)> = self.known_peers
                .iter()
                .filter(|r| {
                    let p = r.value();
                    p.transport != TransportType::BleOnly &&
                    p.ip.is_some() &&
                    p.last_seen.elapsed().as_secs() > PEER_STALE_THRESHOLD_SEC
                })
                .map(|r| {
                    let p = r.value();
                    // 🟢 UPDATED: p.ip เป็น IpAddr แล้ว unwrap ออกมาได้เลย
                    (p.id.clone(), p.ip.unwrap(), p.port, p.display_name.clone())
                })
                .collect();

            if suspects.is_empty() { continue; }

            for (id, ip, port, name) in suspects {
                let peers_ref = self.known_peers.clone();
                let cb_ref = self.callback.clone();

                tokio::spawn(async move {
                    // 🟢 UPDATED: Format รองรับทั้ง IPv4 และ IPv6
                    let addr = if ip.is_ipv6() {
                        format!("[{}]:{}", ip, port)
                    } else {
                        format!("{}:{}", ip, port)
                    };

                    let is_alive = match timeout(Duration::from_secs(2), async {
                        let mut stream = TcpStream::connect(&addr).await?;
                        stream.write_u8(0xFF).await?;
                        let mut buf = [0u8; 1];
                        let n = stream.read(&mut buf).await?;
                        if n > 0 && buf[0] == 0xFF { Ok(()) } else { Err(std::io::Error::new(std::io::ErrorKind::Other, "Bad Pong")) }
                    }).await { Ok(Ok(_)) => true, _ => false };

                    if let Some(mut peer) = peers_ref.get_mut(&id) {
                        if is_alive {
                            peer.last_seen = Instant::now();
                            peer.missed_pings = 0;
                            debug!("✅ Peer Verified: {}", name);
                        } else {
                            peer.missed_pings += 1;
                            warn!("⚠️ Missed Ping {}/3 for {}", peer.missed_pings, name);

                            if peer.missed_pings >= 3 {
                                if peer.transport == TransportType::Hybrid {
                                    info!("🔻 Link Degraded: {} (Fallback to BLE)", name);
                                    peer.transport = TransportType::BleOnly;
                                    peer.ip = None;
                                } else if peer.transport == TransportType::Lan {
                                    info!("💀 Peer Lost: {}", name);
                                    cb_ref.on_peer_lost(&id);
                                    drop(peer); 
                                    peers_ref.remove(&id);
                                }
                            }
                        }
                    }
                });

                let jitter = rand::thread_rng().gen_range(50..150);
                tokio::time::sleep(Duration::from_millis(jitter)).await;
            }
        }
    }

    pub async fn start(&self, device_id: String, port: u16, dev_mode: bool, mut rx: mpsc::Receiver<DiscoveryInternalEvent>) -> anyhow::Result<()> {

        let my_system_name = utils::get_system_name();
        info!("🚀 Discovery Engine Starting: {} (DevMode: {})", my_system_name, dev_mode);

        self.spawn_mdns_listener(device_id.clone(), port, my_system_name.clone(), dev_mode)?;
        self.spawn_ble_listener(device_id.clone(), dev_mode).await?;

        let peers = self.known_peers.clone();
        let cb = self.callback.clone();

        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    DiscoveryInternalEvent::MdnsFound { id, name, ip, port } => {
                        // 🟢 UPDATED: แปลง String กลับเป็น IpAddr เพื่อความปลอดภัย
                        if let Ok(parsed_ip) = ip.parse::<IpAddr>() {
                            peers.entry(id.clone())
                                .and_modify(|peer| {
                                    peer.ip = Some(parsed_ip); // Store as IpAddr
                                    peer.port = port;
                                    peer.last_seen = Instant::now();
                                    peer.missed_pings = 0;

                                    if peer.transport == TransportType::BleOnly {
                                        info!("🆙 Link Upgraded: {} (BLE -> Hybrid)", name);
                                        peer.transport = TransportType::Hybrid;
                                    } else {
                                        peer.transport = TransportType::Lan;
                                    }
                                    cb.on_peer_found(&id, &peer.display_name, &ip, port, peer.ssid.as_deref(), &peer.transport.to_string());
                                })
                                .or_insert_with(|| {
                                    info!("✨ LAN Found: {} @ {}", name, ip);
                                    cb.on_peer_found(&id, &name, &ip, port, None, "LAN");
                                    PeerInfo {
                                        id: id.clone(),
                                        name: name.clone(),
                                        display_name: name,
                                        ip: Some(parsed_ip), // Store as IpAddr
                                        port,
                                        ssid: None,
                                        ble_mac: None,
                                        transport: TransportType::Lan,
                                        last_seen: Instant::now(),
                                        missed_pings: 0,
                                    }
                                });
                        }
                    },

                    DiscoveryInternalEvent::BleFound { id, name, ssid, mac } => {
                        if let Some(mut peer) = peers.get_mut(&id) {
                            peer.ssid = ssid.clone();
                            peer.ble_mac = Some(mac.clone());
                            peer.last_seen = Instant::now();
                            if peer.transport == TransportType::Lan {
                                peer.transport = TransportType::Hybrid;
                                info!("🔗 Link Merged: {} (Hybrid)", name);
                            }
                        } else {
                            info!("👻 BLE Found: {} (Mac: {})", name, mac);
                            cb.on_peer_found(&id, &name, "", 0, ssid.as_deref(), "BLE");
                            peers.insert(id.clone(), PeerInfo {
                                id,
                                name: name.clone(),
                                display_name: name,
                                ip: None,
                                port: 0,
                                ssid,
                                ble_mac: Some(mac),
                                transport: TransportType::BleOnly,
                                last_seen: Instant::now(),
                                missed_pings: 0,
                            });
                        }
                    },

                    DiscoveryInternalEvent::MdnsLost { id } => {
                        let mut remove = false;
                        if let Some(mut peer) = peers.get_mut(&id) {
                            if peer.transport == TransportType::Hybrid {
                                info!("⚠️ LAN Lost, downgrading to BLE: {}", peer.display_name);
                                peer.transport = TransportType::BleOnly;
                                peer.ip = None;
                            } else {
                                remove = true;
                            }
                        }
                        if remove {
                            if peers.remove(&id).is_some() {
                                cb.on_peer_lost(&id);
                            }
                        }
                    },
                }
            }
        });

        Ok(())
    }

    fn spawn_mdns_listener(&self, my_id: String, port: u16, my_name: String, dev_mode: bool) -> anyhow::Result<()> {
        let tx = self.event_tx.clone();
        let daemon = self.daemon.clone();
        let my_ip = Self::get_local_ip();

        let service_type = "_droptea._tcp.local.";
        let instance_name = format!("DropTea-{}", my_id);
        let host_name = format!("{}.local.", my_id);

        let mut properties = HashMap::new();
        properties.insert("id".to_string(), my_id.clone());
        properties.insert("ver".to_string(), "1.0".to_string());
        properties.insert("name".to_string(), my_name);

        let my_info = ServiceInfo::new(
            service_type, &instance_name, &host_name, &my_ip, port, properties
        ).context("Failed to create ServiceInfo")?;

        daemon.register(my_info).context("Failed to register mDNS")?;
        let receiver = daemon.browse(service_type).context("Failed to browse mDNS")?;

        std::thread::spawn(move || {
            while let Ok(event) = receiver.recv() {
                match event {
                    ServiceEvent::ServiceResolved(info) => {
                        if !dev_mode && info.get_fullname().contains(&my_id) { continue; }

                        // 🟢 UPDATED: พยายามหา IPv4 ก่อนเป็นอันดับแรก เพื่อความเสถียรกับ Simulator
                        let best_ip = info.get_addresses().iter()
                            .find(|ip| ip.is_ipv4())          
                            .or_else(|| info.get_addresses().iter().next()); 

                        if let Some(ip) = best_ip {
                            let id = info.get_fullname().to_string();
                            
                            // 🟢 UPDATED: จัด Format IP ให้เป็น String ที่ถูกต้อง (เติม [] ถ้าเป็น IPv6)
                            let ip_str = if ip.is_ipv6() {
                                format!("[{}]", ip)
                            } else {
                                ip.to_string()
                            };
                            
                            let port = info.get_port();
                            let props = info.get_properties();
                            let raw_name = props.get("name").map(|v| v.to_string()).unwrap_or_else(|| "Unknown".to_string());
                            let clean_name = raw_name.split('=').last().unwrap_or(&raw_name).trim().to_string();

                            // 🟢 UPDATED: อนุญาตให้ connect ตัวเองได้ถ้าเป็น Dev Mode (สำหรับ Simulator)
                            // เช็คโดยการถอด [] ออกก่อนเทียบ
                            let clean_ip_str = ip_str.replace(&['[', ']'][..], "");
                            if !dev_mode && clean_ip_str == my_ip { continue; }
                            
                            let _ = tx.blocking_send(DiscoveryInternalEvent::MdnsFound { id, name: clean_name, ip: ip_str, port });
                        }
                    },
                    ServiceEvent::ServiceRemoved(_type, fullname) => {
                         let _ = tx.blocking_send(DiscoveryInternalEvent::MdnsLost { id: fullname });
                    },
                    _ => {}
                }
            }
        });
        Ok(())
    }

    async fn spawn_ble_listener(&self, my_id: String, dev_mode: bool) -> anyhow::Result<()> {
        let tx = self.event_tx.clone();

        // 🟢 Branch 1: Dev Mode (Mock Data)
        if dev_mode {
            tokio::spawn(async move {
                let mut counter = 0;
                loop {
                    if counter >= 5 { break; }
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    counter += 1;
                    let mock_id = format!("{}_mock_{}", my_id, counter);
                    info!("🔧 [DevMode] Injecting Mock BLE Peer");
                    let _ = tx.send(DiscoveryInternalEvent::BleFound {
                        id: mock_id,
                        name: format!("Mock Device #{}", counter),
                        ssid: Some("Dev_WiFi_5G".to_string()),
                        mac: "00:11:22:AA:BB:CC".to_string(),
                    }).await;
                }
            });
            return Ok(());
        }

        // 🟠 Branch 2: Production Mode (Real BLE)
        tokio::spawn(async move {
            let manager = match Manager::new().await { Ok(m) => m, Err(e) => { error!("BLE Init Error: {}", e); return; } };
            let adapters = match manager.adapters().await { Ok(a) => a, Err(e) => { error!("BLE Adapter Error: {}", e); return; } };
            let central = match adapters.into_iter().nth(0) { Some(c) => c, None => { error!("BLE: No Adapter Found"); return; } };

            let mut events = match central.events().await {
                Ok(e) => e,
                Err(e) => { error!("Failed to subscribe to BLE events: {}", e); return; }
            };

            if let Err(e) = central.start_scan(ScanFilter::default()).await {
                error!("BLE Start Scan Error: {}", e);
                return;
            }

            info!("🔵 BLE Scanner Running (Filtering for '{}' or '{}')", DROPTEA_NAME_PREFIX, DROPTEA_UUID_PART);

            let mut processed_cache: HashMap<String, Instant> = HashMap::new();

            while let Some(event) = events.next().await {
                match event {
                    CentralEvent::DeviceDiscovered(id) | CentralEvent::DeviceUpdated(id) => {
                         let id_str = id.to_string();
                         if let Some(last_time) = processed_cache.get(&id_str) {
                             if last_time.elapsed().as_millis() < BLE_CACHE_TTL_MS {
                                 continue; 
                             }
                         }
                         processed_cache.insert(id_str.clone(), Instant::now());

                        if let Ok(p) = central.peripheral(&id).await {
                            if let Ok(Some(props)) = p.properties().await {
                                let name = props.local_name.clone().unwrap_or("Unknown".to_string());
                                let mac = p.address().to_string();
                                let services = props.services.clone();

                                let mut is_target = false;

                                if name.starts_with(DROPTEA_NAME_PREFIX) {
                                    is_target = true;
                                }

                                if !is_target {
                                    for uuid in &services {
                                        if uuid.to_string().to_lowercase().contains(DROPTEA_UUID_PART) {
                                            is_target = true;
                                            break;
                                        }
                                    }
                                }

                                if is_target {
                                    let display_name = if name == "Unknown" {
                                        "iPad/iPhone (DropTea)".to_string()
                                    } else {
                                        name.clone()
                                    };

                                    let unique_id = if name == "Unknown" || name == display_name {
                                        format!("ble-{}", mac.replace(":", ""))
                                    } else {
                                        name.clone()
                                    };

                                    let _ = tx.send(DiscoveryInternalEvent::BleFound {
                                        id: unique_id,
                                        name: display_name,
                                        ssid: None,
                                        mac: mac,
                                    }).await;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        });

        Ok(())
    }
}