use serde::{Serialize, Deserialize};
use tokio::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use async_trait::async_trait;

pub const ACK_SIZE: usize = 9;
pub const MAX_HEADER_SIZE: usize = 64 * 1024;
pub const IO_TIMEOUT: Duration = Duration::from_secs(60);
pub const USER_DECISION_TIMEOUT: Duration = Duration::from_secs(120);
pub const NOTIFY_INTERVAL_MS: u128 = 100;
pub const PIPELINE_BUFFER_SIZE: usize = 4 * 1024 * 1024;
pub const CHANNEL_CAPACITY: usize = 32; 

pub trait DataStream: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> DataStream for T {}

#[async_trait]
pub trait Transport: Send + Sync + 'static {
    type Stream: DataStream;
    async fn accept(&self) -> anyhow::Result<(Self::Stream, std::net::SocketAddr)>;
    async fn connect(&self, ip: &str, port: u16) -> anyhow::Result<Self::Stream>;
    
    // 🟢 UPDATED: เพิ่มฟังก์ชันดึง Port จริงที่ OS สุ่มให้
    fn local_port(&self) -> u16;
}

pub type DynStream = Box<dyn DataStream>;
pub type DynTransport = dyn Transport<Stream = DynStream>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CertificateAction { Accept, Reject }

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileHeader {
    pub filename: String,
    pub filesize: u64,
    pub sender_name: String,
    pub sender_device: String,
    
    #[serde(default)] 
    pub compression: Option<String>, 
}

pub trait TransferCallback: Send + Sync {
    fn on_start(&self, task_id: &str, filename: &str);
    fn on_progress(&self, task_id: &str, current: u64, total: u64);
    fn on_complete(&self, task_id: &str, info: &str);
    fn on_error(&self, task_id: &str, error: &str);
    fn on_reject(&self, task_id: &str, reason: &str);
    fn on_peer_found(&self, id: &str, name: &str, ip: &str, port: u16, ssid: Option<&str>, transport: &str);
    fn on_peer_lost(&self, id: &str);
    fn ask_accept_file(&self, task_id: &str, filename: &str, filesize: u64, sender_name: &str, sender_device: &str) -> anyhow::Result<bool>;
    fn ask_verify_certificate(&self, peer_id: &str, fingerprint: &str, filename: Option<&str>) -> anyhow::Result<CertificateAction>;
}

pub fn pack_ack(status: u8, offset: u64) -> Vec<u8> {
    let mut buf = Vec::with_capacity(ACK_SIZE);
    buf.push(status);
    buf.extend_from_slice(&offset.to_le_bytes());
    buf
}

pub fn unpack_ack(data: &[u8]) -> anyhow::Result<(u8, u64)> {
    if data.len() < ACK_SIZE { return Err(anyhow::anyhow!("ACK too short")); }
    let mut offset_buf = [0u8; 8];
    offset_buf.copy_from_slice(&data[1..9]);
    Ok((data[0], u64::from_le_bytes(offset_buf)))
}

pub async fn copy_pipeline<R, W, F>(mut reader: R, mut writer: W, total: u64, mut on_progress: F) -> anyhow::Result<()> 
where R: AsyncReadExt + Unpin + Send + 'static, W: AsyncWriteExt + Unpin, F: FnMut(u64, u64) + Send + 'static
{
    let (data_tx, mut data_rx) = mpsc::channel::<anyhow::Result<Vec<u8>>>(CHANNEL_CAPACITY);
    let (recycle_tx, mut recycle_rx) = mpsc::channel::<Vec<u8>>(CHANNEL_CAPACITY);
    for _ in 0..CHANNEL_CAPACITY { let _ = recycle_tx.send(vec![0u8; PIPELINE_BUFFER_SIZE]).await; }
    
    let producer_handle = tokio::spawn(async move {
        loop {
            let mut buf = match recycle_rx.recv().await { Some(b) => b, None => break };
            buf.resize(PIPELINE_BUFFER_SIZE, 0);
            match tokio::time::timeout(IO_TIMEOUT, reader.read(&mut buf)).await {
                Ok(Ok(0)) => break, 
                Ok(Ok(n)) => { buf.truncate(n); if data_tx.send(Ok(buf)).await.is_err() { break; } },
                Ok(Err(e)) => { let _ = data_tx.send(Err(anyhow::Error::new(e))).await; break; },
                Err(_) => { let _ = data_tx.send(Err(anyhow::anyhow!("Read Timeout"))).await; break; }
            }
        }
    });

    let mut uploaded = 0u64;
    let mut last_rep = 0u64;
    let mut last_time = tokio::time::Instant::now();
    while let Some(result) = data_rx.recv().await {
        let chunk = result?; 
        tokio::time::timeout(IO_TIMEOUT, writer.write_all(&chunk)).await.map_err(|_| anyhow::anyhow!("Write timeout"))??;
        uploaded += chunk.len() as u64;
        let now = tokio::time::Instant::now();
        if (uploaded - last_rep >= (1024*1024) && now.duration_since(last_time).as_millis() > NOTIFY_INTERVAL_MS) || uploaded == total {
            on_progress(uploaded, total); last_rep = uploaded; last_time = now;
        }
        let _ = recycle_tx.send(chunk).await;
    }
    
    if let Err(e) = producer_handle.await {
        return Err(anyhow::anyhow!("Producer task panic: {}", e));
    }
    Ok(())
}