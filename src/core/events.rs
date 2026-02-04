use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransferEvent {
    Log { level: String, msg: String },
    ServerStarted { port: u16 },
    Error { task_id: String, error: String },
    
    Incoming { task_id: String, filename: String },
    Started { task_id: String, msg: String },
    Progress { task_id: String, current: u64, total: u64 },
    Completed { task_id: String, info: String },
    Rejected { task_id: String, reason: String },

    DiscoveryStarted,
    // 🔥 Updated Event
    PeerFound { 
        id: String, 
        name: String, 
        ip: String, 
        port: u16, 
        ssid: Option<String>, 
        transport: String 
    },
    PeerLost { id: String },
}

pub trait TransferEventHandler: Send + Sync {
    fn on_event(&self, event: TransferEvent);
}