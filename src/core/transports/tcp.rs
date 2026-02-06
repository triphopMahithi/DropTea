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
        _port: u16, 
        storage_path: &str, 
        node_name: &str,
        config: Option<TcpConfig>
    ) -> anyhow::Result<Self> {
        
        let config = config.unwrap_or_default();
        
        // 🟢 UPDATED: Bind Port 0 (OS สุ่มให้)
        let listener = TcpListener::bind("0.0.0.0:0").await?;
        
        let (server_cfg, client_cfg) = security::build_tls_configs(storage_path, node_name)?;
        
        Ok(Self {
            listener,
            acceptor: TlsAcceptor::from(Arc::new(server_cfg)),
            connector: TlsConnector::from(Arc::new(client_cfg)),
            config,
        })
    }

    fn apply_socket_tuning(&self, stream: &TcpStream) -> anyhow::Result<()> {
        crate::core::utils::apply_wifi_tuning(stream)?;
        Ok(())
    }
}

#[async_trait]
impl Transport for TcpTransport {
    type Stream = Box<dyn DataStream>;

    async fn accept(&self) -> anyhow::Result<(Self::Stream, std::net::SocketAddr)> {
        let (stream, addr) = self.listener.accept().await?;
        
        if let Err(e) = self.apply_socket_tuning(&stream) {
            log::warn!("Failed to tune accepted TCP socket: {}", e);
        }

        let tls_stream = self.acceptor.accept(stream).await?;
        Ok((Box::new(tls_stream), addr))
    }

    async fn connect(&self, ip: &str, port: u16) -> anyhow::Result<Self::Stream> {
        let stream = TcpStream::connect((ip, port)).await?;
        self.apply_socket_tuning(&stream)?;

        let domain = tokio_rustls::rustls::ServerName::try_from(ip)
            .or_else(|_| tokio_rustls::rustls::ServerName::try_from("droptea.p2p"))?;
            
        let tls_stream = self.connector.connect(domain, stream).await?;
        Ok(Box::new(tls_stream))
    }

    // 🟢 UPDATED: คืนค่า Port จริง
    fn local_port(&self) -> u16 {
        self.listener.local_addr().map(|a| a.port()).unwrap_or(0)
    }
}