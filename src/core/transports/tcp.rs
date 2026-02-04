use crate::core::transfer::{Transport, DataStream};
use crate::core::security;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;

// --- Constants & Configuration ---

#[derive(Debug, Clone)]
pub struct TcpConfig {
    pub nodelay: bool,
    pub keepalive_duration: Option<Duration>,
}

impl Default for TcpConfig {
    fn default() -> Self {
        Self {
            // NoDelay = true คือสิ่งสำคัญที่สุดสำหรับ App รับส่งไฟล์แบบ Realtime/Interactive
            nodelay: true, 
            keepalive_duration: Some(Duration::from_secs(60)),
        }
    }
}

// --- Transport Implementation ---

pub struct TcpTransport {
    listener: TcpListener,
    acceptor: TlsAcceptor,
    connector: TlsConnector,
    config: TcpConfig, 
}

impl TcpTransport {
    pub async fn new(
        port: u16, 
        storage_path: &str, 
        node_name: &str,
        config: Option<TcpConfig> // รับ Config
    ) -> anyhow::Result<Self> {
        
        let config = config.unwrap_or_default();
        let addr = format!("0.0.0.0:{}", port);
        let listener = TcpListener::bind(&addr).await?;
        
        let (server_cfg, client_cfg) = security::build_tls_configs(storage_path, node_name)?;
        
        Ok(Self {
            listener,
            acceptor: TlsAcceptor::from(Arc::new(server_cfg)),
            connector: TlsConnector::from(Arc::new(client_cfg)),
            config,
        })
    }

    // 🔥 TUNING STEP 2: Helper function สำหรับจูน Socket
    fn apply_socket_tuning(&self, stream: &TcpStream) -> anyhow::Result<()> {
        // เรียกใช้ Tuning Logic จาก utils (ที่ใช้ socket2)
        // สิ่งนี้จะตั้งค่า Buffer Size 2MB และ NoDelay
        crate::core::utils::apply_wifi_tuning(stream)?;

        // Optional: KeepAlive
        // (ปกติ socket2 ตั้ง keepalive ได้ แต่ถ้าอยากใช้ tokio-native ก็ทำตรงนี้เสริมได้)
        
        Ok(())
    }
}

#[async_trait]
impl Transport for TcpTransport {
    type Stream = Box<dyn DataStream>;

    async fn accept(&self) -> anyhow::Result<(Self::Stream, std::net::SocketAddr)> {
        let (stream, addr) = self.listener.accept().await?;
        
        // 🔥 Apply Tuning ทันทีที่รับ Connection
        if let Err(e) = self.apply_socket_tuning(&stream) {
            log::warn!("Failed to tune accepted TCP socket: {}", e);
        }

        let tls_stream = self.acceptor.accept(stream).await?;
        Ok((Box::new(tls_stream), addr))
    }

    async fn connect(&self, ip: &str, port: u16) -> anyhow::Result<Self::Stream> {
        let stream = TcpStream::connect((ip, port)).await?;
        
        // 🔥 Apply Tuning ทันทีที่ Connect ติด
        self.apply_socket_tuning(&stream)?;

        let domain = tokio_rustls::rustls::ServerName::try_from(ip)
            .or_else(|_| tokio_rustls::rustls::ServerName::try_from("droptea.p2p"))?;
            
        let tls_stream = self.connector.connect(domain, stream).await?;
        Ok(Box::new(tls_stream))
    }
}