use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::env;
use tokio::sync::{Semaphore, mpsc};
use tokio::time::{timeout};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::fs::{self as tokio_fs, File as AsyncFile, OpenOptions};
use anyhow::{Context, bail};
use log::info;

use crate::core::transfer::{
    FileHeader, TransferCallback, DataStream, pack_ack, copy_pipeline,
    MAX_HEADER_SIZE, IO_TIMEOUT, USER_DECISION_TIMEOUT, ACK_SIZE,
};
use crate::core::utils::get_unique_path;
use crate::core::notification::{self, UserResponse};
use crate::core::security;
// 🔥 Import โมดูลใหม่
use crate::core::compression::{Compressor, Decompressor, CompressionAlgo};

const IO_BUFFER_SIZE: usize = 1024 * 1024; 

pub async fn handle_incoming<S, CB>(
    mut stream: S,
    save_path: String,
    callback: CB,
    limiter: Arc<Semaphore>,
    pending_map: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<UserResponse>>>>,
) -> anyhow::Result<()>
where 
    S: DataStream, 
    CB: TransferCallback + Clone + 'static, 
{
    // 1. Read Header Size
    let mut len_buf = [0u8; 4];
    // 1. อ่านขนาด Header และดักจับ Ghost Connection
    match timeout(IO_TIMEOUT, stream.read_exact(&mut len_buf)).await {
        Ok(Ok(_)) => {}, 
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
            log::debug!("Ghost connection detected (Early EOF). Ignoring.");
            return Ok(()); 
        },
        Ok(Err(e)) => return Err(anyhow::Error::new(e)),
        Err(_) => return Ok(()), 
    };

    let header_len = u32::from_le_bytes(len_buf) as usize;
    if header_len > MAX_HEADER_SIZE { bail!("Header too large"); }

    // 2. อ่าน Header Body (ไปต่อได้เลย ไม่ต้องอ่าน len_buf ซ้ำแล้ว) 
    // timeout(IO_TIMEOUT, stream.read_exact(&mut len_buf)).await.context("Header size timeout")??;
    // let header_len = u32::from_le_bytes(len_buf) as usize;
    // if header_len > MAX_HEADER_SIZE { bail!("Header too large"); }

    // 2. Read Header Body
    let mut header_buf = vec![0u8; header_len];
    timeout(IO_TIMEOUT, stream.read_exact(&mut header_buf)).await.context("Header read timeout")??;
    let header: FileHeader = serde_json::from_slice(&header_buf).context("Invalid header JSON")?;
    let task_id = header.filename.clone();

    // 3. Rate Limit Check
    let _permit = match limiter.try_acquire() {
        Ok(p) => p,
        Err(_) => {
            let _ = timeout(IO_TIMEOUT, stream.write_all(&pack_ack(0, 0))).await;
            callback.on_reject(&task_id, "System Busy");
            return Ok(());
        }
    };

    // 4. Security Check
    let is_trusted = security::is_trusted(&save_path, &header.sender_name);
    let is_accepted = if is_trusted {
        callback.on_start(&task_id, &header.filename); true 
    } else {
        let (tx, mut rx) = mpsc::unbounded_channel();
        { if let Ok(mut map) = pending_map.lock() { map.insert(task_id.clone(), tx.clone()); } }
        let _ = callback.ask_accept_file(&task_id, &header.filename, header.filesize, &header.sender_name, &header.sender_device);
        let decision = timeout(USER_DECISION_TIMEOUT, rx.recv()).await;
        { if let Ok(mut map) = pending_map.lock() { map.remove(&task_id); } }
        match decision { Ok(Some(UserResponse::Accept)) => { security::add_trust(&save_path, header.sender_name.clone()); true }, _ => false }
    };

    if !is_accepted {
        let _ = timeout(IO_TIMEOUT, stream.write_all(&pack_ack(0, 0))).await;
        callback.on_reject(&task_id, "User Rejected");
        return Ok(());
    }

    // 5. Prepare File
    let final_path = get_unique_path(&save_path, &header.filename);
    let temp_path = final_path.with_extension("part");
    let file = OpenOptions::new().write(true).create(true).truncate(true).open(&temp_path).await?;
    let mut buffered_file = BufWriter::with_capacity(IO_BUFFER_SIZE, file);
    
    // 6. Send ACK
    stream.write_all(&pack_ack(1, 0)).await?;
    
    // 🔥 7. Auto Detect Compression (ถ้า Header บอกว่า none ก็รับสด, ถ้า zstd ก็แกะ)
    let algo = header.compression
        .as_deref()
        .and_then(CompressionAlgo::from_str)
        .unwrap_or(CompressionAlgo::Zstd);

    info!("Receiving '{}' (Mode: {:?})", header.filename, algo);

    let decoder = Decompressor::new(stream, algo);
    let tid = task_id.clone();
    let cb = callback.clone();
    
    match copy_pipeline(decoder, &mut buffered_file, header.filesize, move |c, t| cb.on_progress(&tid, c, t)).await {
        Ok(_) => {
            buffered_file.flush().await?;
            let inner = buffered_file.into_inner(); inner.sync_all().await?;
            tokio_fs::rename(&temp_path, &final_path).await?;
            callback.on_complete(&task_id, &final_path.to_string_lossy());
            Ok(())
        },
        Err(e) => {
            let _ = tokio_fs::remove_file(&temp_path).await;
            Err(e)
        }
    }
}

pub async fn handle_sending<S>(
    mut stream: S,
    path: String,
    task_id: String,
    callback: impl TransferCallback + Clone + 'static,
    my_device_name: String,
    target_os: Option<String>,
) -> anyhow::Result<()> 
where S: DataStream
{
    let file = AsyncFile::open(&path).await.context("Failed to open source file")?;
    let metadata = file.metadata().await?;
    let total_size = metadata.len();
    let filename = std::path::Path::new(&path).file_name().unwrap().to_string_lossy().to_string();
    
    // 🔥 FIXED: เลือกโหมดการส่ง (ถ้าเป็น iOS ให้ส่งสด)
    let compression_algo = match target_os.as_deref() {
        Some("ios") => CompressionAlgo::None, // ส่งสด (Raw)
        _ => CompressionAlgo::Zstd,           // ส่ง Zstd (Default)
    };

    info!("Sending '{}' to {:?} (Mode: {:?})", filename, target_os, compression_algo);

    let header = FileHeader { 
        filename, 
        filesize: total_size, 
        sender_name: my_device_name, 
        sender_device: env::consts::OS.to_string(),
        compression: Some(compression_algo.as_str().to_string())
    };
    
    let json = serde_json::to_vec(&header).context("Failed to serialize header")?;
    stream.write_all(&(json.len() as u32).to_le_bytes()).await?;
    stream.write_all(&json).await?;

    let mut ack = vec![0u8; ACK_SIZE];
    match timeout(USER_DECISION_TIMEOUT, stream.read_exact(&mut ack)).await {
        Ok(Ok(_)) => {},
        _ => { callback.on_reject(&task_id, "Timeout"); return Ok(()); }
    };
    if ack[0] == 0 { callback.on_reject(&task_id, "Receiver Rejected"); return Ok(()); }

    callback.on_start(&task_id, &header.filename);

    // 🔥 ใช้ Compressor Factory
    let mut encoder = Compressor::new(stream, compression_algo);
    let tid = task_id.clone();
    let cb = callback.clone();
    
    copy_pipeline(
        BufReader::with_capacity(IO_BUFFER_SIZE, file), 
        &mut encoder, 
        total_size, 
        move |c, t| cb.on_progress(&tid, c, t)
    ).await?;
    
    encoder.shutdown().await?;
    callback.on_complete(&task_id, "Success");
    Ok(())
}