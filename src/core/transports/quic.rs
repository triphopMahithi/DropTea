use crate::core::transfer::{Transport, DataStream};
use crate::core::security;
use quinn::{Endpoint, RecvStream, SendStream, Connection, TransportConfig, VarInt};
use async_trait::async_trait;
use std::sync::Arc;
use std::net::SocketAddr;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::collections::HashMap;
use tokio::sync::RwLock; // ✅ เปลี่ยนใช้ RwLock เพื่อ High Concurrency
use std::time::Duration;

// --- Constants & Configuration ---

pub const PROTOCOL_SERVER_NAME: &str = "droptea.p2p";
pub const PROTOCOL_ALPN: &[&[u8]] = &[b"droptea-p2p"];

#[derive(Debug, Clone)]
pub struct QuicConfig {
    pub stream_window_size: u64,
    pub connection_window_size: u64,
    pub max_concurrent_streams: u32, // ✅ เพิ่ม Config สำหรับ Parallelism
    pub keep_alive_interval: Duration,
    pub max_idle_timeout: Duration,
}

impl Default for QuicConfig {
    fn default() -> Self {
        Self {
            // ✅ High Throughput Tuning: ขยาย Window ให้ใหญ่สู้ TCP
            stream_window_size: 32 * 1024 * 1024,      // 32 MB per stream
            connection_window_size: 128 * 1024 * 1024, // 128 MB total
            max_concurrent_streams: 1000,              // ✅ รองรับ 1000 streams พร้อมกัน
            keep_alive_interval: Duration::from_secs(5),
            max_idle_timeout: Duration::from_secs(60),
        }
    }
}

// --- Data Stream Wrapper ---

pub struct QuicDataStream {
    send: SendStream,
    recv: RecvStream,
}

impl AsyncRead for QuicDataStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.recv).poll_read(cx, buf)
    }
}

impl AsyncWrite for QuicDataStream {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.send).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.send).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.send).poll_shutdown(cx)
    }
}

// --- Transport Implementation ---

pub struct QuicTransport {
    endpoint: Endpoint,
    // ✅ ใช้ RwLock: อ่านได้หลาย thread พร้อมกัน, เขียนทีละ thread
    connections: Arc<RwLock<HashMap<SocketAddr, Connection>>>,
}

impl QuicTransport {
    pub async fn new(
        port: u16, 
        storage_path: &str, 
        node_name: &str, 
        config: Option<QuicConfig>
    ) -> anyhow::Result<Self> {
        
        let config = config.unwrap_or_default();
        let (certs, key) = security::load_or_generate_identity(storage_path, node_name)?;
        let sec_path = std::path::Path::new(storage_path).join("security");

        // 1. Setup Transport Config (Performance Tuning)
        let mut transport_config = TransportConfig::default();
        
        // Window Sizes & Flow Control
        transport_config.stream_receive_window(VarInt::try_from(config.stream_window_size).unwrap_or(VarInt::MAX));
        transport_config.receive_window(VarInt::try_from(config.connection_window_size).unwrap_or(VarInt::MAX));
        
        // Parallelism Limits
        transport_config.max_concurrent_uni_streams(VarInt::try_from(config.max_concurrent_streams).unwrap_or(VarInt::from_u32(100)));
        transport_config.max_concurrent_bidi_streams(VarInt::try_from(config.max_concurrent_streams).unwrap_or(VarInt::from_u32(100)));

        transport_config.keep_alive_interval(Some(config.keep_alive_interval));
        transport_config.max_idle_timeout(Some(config.max_idle_timeout.try_into()?));
        
        // Optimization: Disable Datagram buffer if not used (Save Memory/CPU)
        transport_config.datagram_receive_buffer_size(None);

        let transport_config_arc = Arc::new(transport_config);

        // 2. Setup Server Config
        let mut server_crypto = rustls::ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;
        
        server_crypto.alpn_protocols = PROTOCOL_ALPN.iter().map(|&x| x.to_vec()).collect();
        
        let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(server_crypto));
        server_config.transport_config(transport_config_arc.clone());
        
        // 3. Setup Client Config
        let mut client_crypto = rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_custom_certificate_verifier(security::TofuVerifier::new(
            security::SecurityManager::new(sec_path) 
            ))
            .with_no_client_auth();
            
        client_crypto.alpn_protocols = PROTOCOL_ALPN.iter().map(|&x| x.to_vec()).collect();
        
        let mut client_config = quinn::ClientConfig::new(Arc::new(client_crypto));
        client_config.transport_config(transport_config_arc);

        // 4. Create Endpoint
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        let mut endpoint = Endpoint::server(server_config, addr)?;
        endpoint.set_default_client_config(client_config);

        Ok(Self { 
            endpoint,
            connections: Arc::new(RwLock::new(HashMap::new())), // ✅ Init RwLock
        })
    }

    // ✅ Logic ใหม่: Double-Checked Locking เพื่อลด Blocking I/O
    async fn get_or_connect(&self, addr: SocketAddr) -> anyhow::Result<Connection> {
        // STEP 1: Fast Path (Read Lock) - เช็คเร็วๆ ว่ามีของไหม
        {
            let conns = self.connections.read().await;
            if let Some(conn) = conns.get(&addr) {
                if conn.close_reason().is_none() {
                    return Ok(conn.clone());
                }
            }
        } // Read Lock ถูกปล่อยตรงนี้ ทันทีที่อ่านเสร็จ

        // STEP 2: Network I/O (Connect) - ทำนอก Lock
        // ตรงนี้คือจุดที่เคยบล็อกระบบ ตอนนี้ทำขนานได้แล้วเพราะไม่มี Lock ค้าง
        let connecting = self.endpoint.connect(addr, PROTOCOL_SERVER_NAME)?;
        let connection = connecting.await?;

        // STEP 3: Slow Path (Write Lock) - บันทึกผล
        {
            let mut conns = self.connections.write().await;
            
            // Double-Check: เช็คซ้ำว่ามีใคร Connect เสร็จตัดหน้าเราไปไหม
            if let Some(existing_conn) = conns.get(&addr) {
                if existing_conn.close_reason().is_none() {
                    // ถ้ามีคนทำเสร็จก่อน เราใช้ของเขา (ทิ้งของเรา) เพื่อความคุ้มค่า
                    return Ok(existing_conn.clone());
                }
            }

            // ถ้าไม่มีจริงๆ ให้ใส่ของเราเข้าไป
            conns.insert(addr, connection.clone());
        } // Write Lock ถูกปล่อยตรงนี้

        Ok(connection)
    }
}

#[async_trait]
impl Transport for QuicTransport {
    type Stream = Box<dyn DataStream>;

    async fn accept(&self) -> anyhow::Result<(Self::Stream, SocketAddr)> {
        let connecting = self.endpoint.accept().await.ok_or(anyhow::anyhow!("Endpoint closed"))?;
        let connection = connecting.await?;
        let addr = connection.remote_address();
        
        let (send, recv) = connection.accept_bi().await?;
        
        Ok((Box::new(QuicDataStream { send, recv }), addr))
    }

    async fn connect(&self, ip: &str, port: u16) -> anyhow::Result<Self::Stream> {
        let addr: SocketAddr = format!("{}:{}", ip, port).parse()?;
        
        // เรียกใช้ Logic ใหม่ (Connection Pooling + Non-blocking)
        let connection = self.get_or_connect(addr).await?;
        
        // เปิด Stream ใหม่บน Connection เดิม (Multiplexing)
        let (send, recv) = connection.open_bi().await?;
        
        Ok(Box::new(QuicDataStream { send, recv }))
    }
}