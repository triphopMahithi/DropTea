use std::sync::{Arc, Mutex as StdMutex}; 
use std::collections::HashMap;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::{Semaphore, Mutex as TokioMutex, mpsc};
use tokio::time::Instant;
use log::{info, error};

use crate::core::events::{TransferEvent, TransferEventHandler};
use crate::core::transfer::{DynTransport, TransferCallback};
use crate::core::handlers::{handle_incoming, handle_sending};
use crate::core::discovery::{DiscoveryEngine, DiscoveryInternalEvent};
use crate::core::transports::tcp::TcpTransport;
use crate::core::transports::quic::QuicTransport;
use crate::core::transports::plain_tcp::PlainTcpTransport;

const MAX_CONCURRENT_CONNECTIONS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TransportMode { Tcp, Quic, PlainTcp }

#[derive(Debug, Clone)]
pub struct DropTeaConfig {
    pub mode: TransportMode,
    pub port: u16,
    pub storage_path: String,
    pub node_name: String,
    pub dev_mode: bool,
}

pub struct ClientStat { pub count: u32, pub first_seen: Instant, pub banned_until: Option<Instant> }
pub struct ConnectionGuard { pub clients: TokioMutex<HashMap<std::net::IpAddr, ClientStat>> }
impl ConnectionGuard {
    pub fn new() -> Self { Self { clients: TokioMutex::new(HashMap::new()) } }
    pub async fn check_access(&self, ip: std::net::IpAddr) -> bool { true }
}

pub struct DropTeaCore {
    pub rt: Arc<Runtime>,
    pub handler: Arc<Box<dyn TransferEventHandler>>,
    pub transport: Arc<DynTransport>,
    pub discovery: DiscoveryEngine<EventHandlerAdapter>,
    pub discovery_rx: StdMutex<Option<mpsc::Receiver<DiscoveryInternalEvent>>>,
    pub guard: Arc<ConnectionGuard>,
    pub outgoing_limiter: Arc<Semaphore>,
    pub incoming_limiter: Arc<Semaphore>, 
    pub pending_transfers: Arc<StdMutex<HashMap<String, mpsc::UnboundedSender<crate::core::notification::UserResponse>>>>,
    pub node_name: String,
    pub dev_mode: bool,
}

#[derive(Clone)]
pub struct EventHandlerAdapter(pub Arc<Box<dyn TransferEventHandler>>);

impl TransferCallback for EventHandlerAdapter {
    fn ask_accept_file(&self, task_id: &str, filename: &str, size: u64, sender: &str, device: &str) -> anyhow::Result<bool> {
        let data = format!("[[REQUEST]]|{}|{}|{}|{}", filename, size, sender, device);
        self.0.on_event(TransferEvent::Incoming { task_id: task_id.to_string(), filename: data });
        Ok(false)
    }
    fn on_start(&self, task_id: &str, filename: &str) { self.0.on_event(TransferEvent::Started { task_id: task_id.to_string(), msg: filename.to_string() }); }
    fn on_progress(&self, task_id: &str, current: u64, total: u64) { self.0.on_event(TransferEvent::Progress { task_id: task_id.to_string(), current, total }); }
    fn on_complete(&self, task_id: &str, info: &str) { self.0.on_event(TransferEvent::Completed { task_id: task_id.to_string(), info: info.to_string() }); }
    fn on_error(&self, task_id: &str, error: &str) { self.0.on_event(TransferEvent::Error { task_id: task_id.to_string(), error: error.to_string() }); }
    fn on_reject(&self, task_id: &str, reason: &str) { self.0.on_event(TransferEvent::Rejected { task_id: task_id.to_string(), reason: reason.to_string() }); }
    fn on_peer_found(&self, id: &str, name: &str, ip: &str, port: u16, ssid: Option<&str>, transport: &str) {
        self.0.on_event(TransferEvent::PeerFound { id: id.to_string(), name: name.to_string(), ip: ip.to_string(), port, ssid: ssid.map(|s| s.to_string()), transport: transport.to_string() });
    }
    fn on_peer_lost(&self, id: &str) { self.0.on_event(TransferEvent::PeerLost { id: id.to_string() }); }
    fn ask_verify_certificate(&self, _: &str, _: &str, _: Option<&str>) -> anyhow::Result<crate::core::transfer::CertificateAction> { Ok(crate::core::transfer::CertificateAction::Accept) }
}

impl DropTeaCore {
    pub fn new_with_config(rt: Arc<Runtime>, config: DropTeaConfig, handler: Box<dyn TransferEventHandler>) -> anyhow::Result<Self> {
        let transport: Arc<DynTransport> = match config.mode {
            TransportMode::Tcp => Arc::new(rt.block_on(async { TcpTransport::new(config.port, &config.storage_path, &config.node_name, None).await })?),
            TransportMode::Quic => Arc::new(rt.block_on(async { QuicTransport::new(config.port, &config.storage_path, &config.node_name, None).await })?),
            TransportMode::PlainTcp => Arc::new(rt.block_on(async { PlainTcpTransport::new(config.port).await })?),
        };

        let h_arc = Arc::new(handler);
        let (discovery, rx) = DiscoveryEngine::new(EventHandlerAdapter(h_arc.clone()))?;
        Ok(Self {
            rt, handler: h_arc, transport, discovery, discovery_rx: StdMutex::new(Some(rx)),
            guard: Arc::new(ConnectionGuard::new()),
            outgoing_limiter: Arc::new(Semaphore::new(50)),
            incoming_limiter: Arc::new(Semaphore::new(5)), 
            pending_transfers: Arc::new(StdMutex::new(HashMap::new())),
            node_name: config.node_name,
            dev_mode: config.dev_mode,
        })
    }

    pub fn start_service(&self, port: u16) {
        let rt = self.rt.clone(); let transport = self.transport.clone(); let h = self.handler.clone(); 
        let guard = self.guard.clone(); let inc_lim = self.incoming_limiter.clone(); let p_map = self.pending_transfers.clone(); 
        let save_path = "./downloads".to_string(); 
        let is_dev = self.dev_mode;
        rt.spawn(async move {
            h.on_event(TransferEvent::ServerStarted { port });
            loop {
                match transport.accept().await {
                    Ok((stream, addr)) => {
                        let h_c = h.clone(); let path = save_path.clone(); let lim = inc_lim.clone(); let map = p_map.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_incoming(stream, path, EventHandlerAdapter(h_c.clone()), lim, map).await {
                                if is_dev {
                                    h_c.on_event(TransferEvent::Error { task_id: "incoming".into(), error: e.to_string() });
                                } else {
                                    log::error!("Incoming connection failed: {}", e);
                            }
                            }
                        });
                    }
                    Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
                }
            }
        });
        
        let rx_opt = self.discovery_rx.lock().unwrap().take();
        if let Some(rx) = rx_opt {
            let discovery = self.discovery.clone();
            let device_id = self.node_name.clone(); 
            let is_dev = self.dev_mode;
            let h_discovery = self.handler.clone();
            rt.spawn(async move {
                if let Err(e) = discovery.start(device_id, port, is_dev, rx).await {
                    h_discovery.on_event(TransferEvent::Error { task_id: "discovery".into(), error: e.to_string() });
                }
            });
        }
    }

    pub fn send_file(&self, ip: String, port: u16, path: String, task_id: String, my_name: String, event_handler: Box<dyn TransferEventHandler>, target_os: Option<String>) {
        let rt = self.rt.clone(); let transport = self.transport.clone();
        let h: Arc<Box<dyn TransferEventHandler>> = Arc::new(event_handler);
        let limiter = self.outgoing_limiter.clone();
        
        rt.spawn(async move {
            let _p = match limiter.acquire().await { Ok(p) => p, Err(_) => return };
            let target_host = if ip.contains(':') && !ip.starts_with('[') { format!("[{}]", ip) } else { ip.clone() };

            match transport.connect(&target_host, port).await {
                Ok(stream) => {
                    let adapter = EventHandlerAdapter(h.clone());
                    if let Err(e) = handle_sending(stream, path, task_id.clone(), adapter, my_name, target_os).await {
                        h.on_event(TransferEvent::Error { task_id, error: e.to_string() });
                    }
                }
                Err(e) => h.on_event(TransferEvent::Error { task_id, error: e.to_string() }),
            }
        });
    }

    pub fn resolve_request(&self, task_id: String, accept: bool) {
        if let Ok(mut map) = self.pending_transfers.lock() {
            if let Some(tx) = map.remove(&task_id) {
                let resp = if accept { crate::core::notification::UserResponse::Accept } else { crate::core::notification::UserResponse::Decline };
                let _ = tx.send(resp);
            }
        }
    }
}