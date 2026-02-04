use anyhow::Context;
use fs2::FileExt;
use std::fs::{self as std_fs, File as StdFile};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;
use whoami;
use zip::write::FileOptions;
use socket2::SockRef; // 🔥 Import socket2

// --- Constants ---
pub const ACK_SIZE: usize = 9;
// ปรับ Buffer Size เป็น 128KB สำหรับการอ่านไฟล์เพื่อ Hash/Compress
// (ส่วน App buffer สำหรับ Pipeline จะแยกไปแก้ใน handlers.rs)
const BUFFER_SIZE: usize = 128 * 1024;

// --- System Info ---
pub fn get_system_name() -> String {
    let username = whoami::username();
    if matches!(
        username.as_str(),
        "user" | "root" | "ubuntu" | "admin" | "raspberry"
    ) {
        return whoami::devicename();
    }
    username
}

// --- 🔧 Network Tuning (ใหม่) ---
// ฟังก์ชันสำหรับจูน Socket ให้เหมาะกับ Wi-Fi (High Bandwidth, High Jitter)
pub fn apply_wifi_tuning(stream: &tokio::net::TcpStream) -> anyhow::Result<()> {
    let socket = SockRef::from(stream);
    
    // 1. ขยาย TCP Buffer (Kernel Level) เป็น 2MB
    // เพื่อรองรับ BDP (Bandwidth-Delay Product) ของ Gigabit Wi-Fi
    socket.set_send_buffer_size(2 * 1024 * 1024)?; 
    socket.set_recv_buffer_size(2 * 1024 * 1024)?;

    // 2. ปิด Nagle's Algorithm (ลด Latency)
    // Wi-Fi มี packet loss บ่อย การรอรวม packet ทำให้ช้าลงโดยไม่จำเป็น
    socket.set_nodelay(true)?;

    Ok(())
}

// --- 📦 File Operations ---

pub fn calculate_quick_hash(path: String, limit: Option<u64>) -> anyhow::Result<Vec<u8>> {
    let f = StdFile::open(&path).context("Failed to open file for hashing")?;
    let mut reader = BufReader::with_capacity(BUFFER_SIZE, f);
    
    let mut h = blake3::Hasher::new();
    let mut buffer = vec![0u8; BUFFER_SIZE]; 
    let mut total_read = 0u64;
    let limit = limit.unwrap_or(u64::MAX);

    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 { break; }

        let remaining = limit.saturating_sub(total_read);
        let take = (n as u64).min(remaining) as usize;

        h.update(&buffer[..take]);
        total_read += take as u64;

        if total_read >= limit { break; }
    }
    
    Ok(h.finalize().as_bytes().to_vec())
}

pub fn compress_folder(folder: String, zip_out: String) -> anyhow::Result<bool> {
    let f = StdFile::create(&zip_out).context("Failed to create zip file")?;
    let mut z = zip::ZipWriter::new(f);
    let folder_path = Path::new(&folder);

    let walk = WalkDir::new(&folder);

    for entry in walk {
        let entry = entry.map_err(|e| anyhow::anyhow!("WalkDir error: {}", e))?;
        let path = entry.path();

        if path.is_dir() { continue; }

        let name = path.strip_prefix(folder_path)?.to_str().unwrap_or("unknown");

        #[cfg(unix)]
        let options = {
            use std::os::unix::fs::PermissionsExt;
            let metadata = std_fs::metadata(path)?;
            FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .unix_permissions(metadata.permissions().mode())
        };

        #[cfg(not(unix))]
        let options = FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        z.start_file(name, options)?;
        let mut f_in = StdFile::open(path)?;
        io::copy(&mut f_in, &mut z)?;
    }
    z.finish()?;
    Ok(true)
}

pub fn extract_zip(zip_path: String, extract_to: String) -> anyhow::Result<bool> {
    let f = StdFile::open(&zip_path)?;
    let mut z = zip::ZipArchive::new(f)?;

    for i in 0..z.len() {
        let mut file = z.by_index(i)?;
        let outpath = match file.enclosed_name() {
            Some(p) => Path::new(&extract_to).join(p),
            None => continue,
        };

        if file.name().ends_with('/') {
            std_fs::create_dir_all(&outpath)?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    std_fs::create_dir_all(p)?;
                }
            }
            let mut outfile = StdFile::create(&outpath)?;
            io::copy(&mut file, &mut outfile)?;
            
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = file.unix_mode() {
                    std_fs::set_permissions(&outpath, std_fs::Permissions::from_mode(mode))?;
                }
            }
        }
    }
    Ok(true)
}

pub fn preallocate_file(path: String, size: u64) -> anyhow::Result<bool> {
    let f = StdFile::create(&path)?;
    f.allocate(size)?;
    Ok(true)
}

pub fn get_unique_path(dir: &str, raw_filename: &str) -> PathBuf {
    let safe_filename = Path::new(raw_filename)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown_file".to_string());

    let base_path = Path::new(dir).join(&safe_filename);
    if !base_path.exists() { return base_path; }

    let stem = Path::new(&safe_filename).file_stem().and_then(|s| s.to_str()).unwrap_or(&safe_filename);
    let ext = Path::new(&safe_filename).extension().and_then(|s| s.to_str()).map(|e| format!(".{}", e)).unwrap_or_default();

    let try_simple = Path::new(dir).join(format!("{}_1{}", stem, ext));
    if !try_simple.exists() { return try_simple; }

    let start = SystemTime::now();
    let since_the_epoch = start.duration_since(UNIX_EPOCH).unwrap_or_default();
    let unique_suffix = since_the_epoch.as_nanos(); 

    Path::new(dir).join(format!("{}_{}{}", stem, unique_suffix, ext))
}

pub fn pack_ack(status: u8, offset: u64) -> Vec<u8> {
    let mut b = Vec::with_capacity(ACK_SIZE);
    b.push(status);
    b.extend_from_slice(&offset.to_le_bytes());
    b
}

pub fn unpack_ack(data: &[u8]) -> anyhow::Result<(u8, u64)> {
    if data.len() < ACK_SIZE {
        return Err(anyhow::anyhow!("ACK data too short: expected {}, got {}", ACK_SIZE, data.len()));
    }
    let mut off_bytes = [0u8; 8];
    off_bytes.copy_from_slice(&data[1..9]);
    Ok((data[0], u64::from_le_bytes(off_bytes)))
}