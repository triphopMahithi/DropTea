use async_trait::async_trait;
use tokio::net::{TcpListener, TcpStream};
use anyhow::Result;
use std::net::SocketAddr;

use crate::core::transfer::{Transport, DynStream};

pub struct PlainTcpTransport {
    listener: TcpListener,
}

impl PlainTcpTransport {
    pub async fn new(port: u16) -> Result<Self> {
        // Bind Port แบบ TCP ปกติ
        let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
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
}