use async_trait::async_trait;
use tokio::net::{TcpListener, TcpStream};
use anyhow::Result;
use std::net::SocketAddr;

use crate::core::transfer::{Transport, DynStream};

pub struct PlainTcpTransport {
    listener: TcpListener,
}

impl PlainTcpTransport {
    pub async fn new(_port: u16) -> Result<Self> {
        // 🟢 UPDATED: Bind Port 0 (ให้ OS สุ่มให้) แทนที่จะใช้ port จาก config
        // เพื่อป้องกันปัญหา Address already in use
        let listener = TcpListener::bind("0.0.0.0:0").await?;
        Ok(Self { listener })
    }
}

#[async_trait]
impl Transport for PlainTcpTransport {
    type Stream = DynStream;

    async fn accept(&self) -> Result<(Self::Stream, SocketAddr)> {
        // รับ Connection เข้ามาแล้วส่งคืน Stream เลย (ไม่ต้อง Handshake TLS)
        let (stream, addr) = self.listener.accept().await?;
        Ok((Box::new(stream), addr))
    }

    async fn connect(&self, ip: &str, port: u16) -> Result<Self::Stream> {
        // เชื่อมต่อไปหาปลายทางแบบ TCP ปกติ
        let stream = TcpStream::connect(format!("{}:{}", ip, port)).await?;
        Ok(Box::new(stream))
    }

    // 🟢 UPDATED: คืนค่า Port จริงที่ OS สุ่มได้
    fn local_port(&self) -> u16 {
        self.listener.local_addr().map(|a| a.port()).unwrap_or(0)
    }
}