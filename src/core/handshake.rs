use btleplug::api::{Central, Manager as _, Peripheral, ScanFilter, WriteType};
use btleplug::platform::Manager;
use uuid::Uuid;
use log::{info, error, warn};
use std::time::Duration;
use tokio::time;

// UUID ของ "กล่องจดหมาย" (Characteristic) ที่เราสร้างใน iPad
const HANDSHAKE_CHAR_UUID: &str = "0000d7eb-0000-1000-8000-00805f9b34fb";

pub async fn connect_and_say_hello(mac_address: String) -> anyhow::Result<()> {
    info!("🔗 Initiating handshake with: {}", mac_address);

    let manager = Manager::new().await?;
    let adapters = manager.adapters().await?;
    let central = adapters.into_iter().nth(0).ok_or(anyhow::anyhow!("No BLE Adapter"))?;

    // 1. ลองหาใน Cache ก่อน
    let mut peripherals = central.peripherals().await?;
    let mut target_device = peripherals.iter()
        .find(|p| p.address().to_string() == mac_address)
        .cloned();

    // 2. ถ้าไม่เจอ ให้เริ่ม Scan ใหม่ (Re-scan logic)
    if target_device.is_none() {
        warn!("⚠️ Device not found in cache. Starting quick scan...");
        
        // เริ่ม Scan
        central.start_scan(ScanFilter::default()).await?;
        
        // รอสูงสุด 5 วินาที
        let start_time = std::time::Instant::now();
        loop {
            time::sleep(Duration::from_millis(500)).await; // เช็คทุก 0.5 วิ
            
            peripherals = central.peripherals().await?;
            target_device = peripherals.iter()
                .find(|p| p.address().to_string() == mac_address)
                .cloned();

            if target_device.is_some() {
                info!("🎉 Found device during re-scan!");
                break;
            }

            if start_time.elapsed().as_secs() > 5 {
                // หมดเวลา
                break;
            }
        }
        
        // (Optional) หยุด Scan เพื่อประหยัดแบตและลดคลื่นรบกวนตอน Connect
        // central.stop_scan().await?; 
    }

    // 3. ถ้ายังไม่เจออีก ก็ยอมแพ้
    let device = target_device.ok_or(anyhow::anyhow!("❌ Device {} unavailable after scan.", mac_address))?;

    // 4. สั่ง Connect
    info!("⏳ Connecting to {}...", mac_address);
    // ลอง Connect (Retry 3 ครั้งเผื่อพลาด)
    let mut connected = false;
    for i in 0..3 {
        match device.connect().await {
            Ok(_) => { connected = true; break; },
            Err(e) => {
                warn!("⚠️ Connect attempt {} failed: {}", i+1, e);
                time::sleep(Duration::from_millis(500)).await;
            }
        }
    }

    if !connected {
        return Err(anyhow::anyhow!("Failed to connect after retries"));
    }

    info!("✅ Connected! Discovering services...");

    // 5. Discover Services
    device.discover_services().await?;

    // 6. หา Characteristic เป้าหมาย (d7eb)
    let chars = device.characteristics();
    let handshake_char = chars.iter().find(|c| c.uuid == Uuid::parse_str(HANDSHAKE_CHAR_UUID).unwrap());

    if let Some(c) = handshake_char {
        info!("📬 Found Handshake Mailbox! Sending 'Hello'...");
        
        let data = "Hello DropTea".as_bytes().to_vec();
        
        // เขียนข้อมูล
        match device.write(c, &data, WriteType::WithoutResponse).await {
            Ok(_) => info!("🚀 Handshake Sent Successfully!"),
            Err(e) => error!("❌ Write Failed: {}", e),
        }
    } else {
        error!("❌ Error: Handshake Characteristic ({}) not found on device.", HANDSHAKE_CHAR_UUID);
        device.disconnect().await?;
        return Err(anyhow::anyhow!("Characteristic not found"));
    }

    // Disconnect เมื่อเสร็จงาน (เพื่อไม่ให้บล็อกการเชื่อมต่ออื่น)
    let _ = device.disconnect().await;
    
    Ok(())
}