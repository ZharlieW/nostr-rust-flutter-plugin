use nostr_relay_builder::{LocalRelay, RelayBuilder};
use nostr_ndb::NdbDatabase;
use std::sync::{Arc, Mutex};
use std::net::IpAddr;
use std::path::PathBuf;
use tokio::runtime::Runtime;

// Global relay instance
static RELAY_INSTANCE: Mutex<Option<Arc<LocalRelay>>> = Mutex::new(None);
static RELAY_CLIENT_URL: Mutex<Option<String>> = Mutex::new(None);
static RUNTIME: Mutex<Option<Arc<Runtime>>> = Mutex::new(None);

/// Relay configuration
#[derive(Debug, Clone)]
pub struct RelayConfig {
    pub host: String,
    pub port: u16,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8081,
        }
    }
}

/// Initialize and start the relay
/// 
/// # Arguments
/// * `host` - IP address to bind (e.g. "127.0.0.1" or "0.0.0.0")
/// * `port` - Port number (e.g. 8081)
/// * `db_path` - Database path (reserved for future persistent storage)
pub fn start_relay(host: String, port: u16, db_path: String) -> Result<String, String> {
    // Initialize tracing with INFO level
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .try_init();
    
    // Get or create runtime
    let runtime = {
        let mut rt_guard = RUNTIME.lock().map_err(|e| format!("Failed to lock runtime: {}", e))?;
        if rt_guard.is_none() {
            let rt = Runtime::new().map_err(|e| format!("Failed to create tokio runtime: {}", e))?;
            *rt_guard = Some(Arc::new(rt));
        }
        rt_guard.as_ref().unwrap().clone()
    };

    // Start relay in the runtime
    let url = runtime.block_on(async {
        start_relay_async(host, port, db_path).await
    })?;

    Ok(url)
}

async fn start_relay_async(host: String, port: u16, db_path: String) -> Result<String, String> {
    // Parse IP address
    let addr: IpAddr = host.parse()
        .map_err(|e| format!("Invalid IP address '{}': {}", host, e))?;
    
    // Create NDB database (nostrdb, persistent, cross-platform)
    // NDB uses a string path instead of PathBuf
    let db_path_str = db_path.clone();
    
    // Create parent directory if it doesn't exist
    let db_path_buf = PathBuf::from(&db_path);
    if let Some(parent) = db_path_buf.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create database directory: {}", e))?;
    }
    
    // Create NDB database (sync operation)
    // NdbDatabase::open expects a string path
    let database = NdbDatabase::open(&db_path_str)
        .map_err(|e| format!("Failed to open NDB database: {}", e))?;
    
    // Build relay
    let builder = RelayBuilder::default()
        .addr(addr)
        .port(port)
        .database(Arc::new(database));
    
    // Start relay
    let relay = LocalRelay::run(builder)
        .await
        .map_err(|e| format!("Failed to start relay: {}", e))?;
    
    let url = relay.url();
    
    // Fix URL: Replace 0.0.0.0 with 127.0.0.1 for client connections
    let client_url = if addr.to_string() == "0.0.0.0" {
        format!("ws://127.0.0.1:{}", port)
    } else {
        url.clone()
    };
    
    // Log relay start (only essential info)
    tracing::info!("Nostr relay started on {}", client_url);
    
    // Store relay instance
    {
        let mut relay_guard = RELAY_INSTANCE.lock()
            .map_err(|e| format!("Failed to lock relay instance: {}", e))?;
        *relay_guard = Some(Arc::new(relay));
    }
    
    // Store client URL
    {
        let mut url_guard = RELAY_CLIENT_URL.lock()
            .map_err(|e| format!("Failed to lock client URL: {}", e))?;
        *url_guard = Some(client_url.clone());
    }
    
    // Return the client-usable URL
    Ok(client_url)
}

/// Stop the relay
pub fn stop_relay() -> Result<(), String> {
    let mut relay_guard = RELAY_INSTANCE.lock()
        .map_err(|e| format!("Failed to lock relay instance: {}", e))?;
    
    if let Some(relay) = relay_guard.take() {
        relay.shutdown();
        
        // Clear client URL
        if let Ok(mut url_guard) = RELAY_CLIENT_URL.lock() {
            *url_guard = None;
        }
        
        Ok(())
    } else {
        Err("Relay is not running".to_string())
    }
}

/// Get relay URL (returns client-usable URL)
pub fn get_relay_url() -> Result<String, String> {
    let url_guard = RELAY_CLIENT_URL.lock()
        .map_err(|e| format!("Failed to lock client URL: {}", e))?;
    
    if let Some(url) = url_guard.as_ref() {
        Ok(url.clone())
    } else {
        Err("Relay is not running".to_string())
    }
}

/// Check if relay is running
pub fn is_relay_running() -> bool {
    if let Ok(guard) = RELAY_INSTANCE.lock() {
        guard.is_some()
    } else {
        false
    }
}

// FFI-compatible functions using flutter_rust_bridge
#[flutter_rust_bridge::frb(sync)]
pub fn relay_start(host: String, port: u16, db_path: String) -> Result<String, String> {
    start_relay(host, port, db_path)
}

#[flutter_rust_bridge::frb(sync)]
pub fn relay_stop() -> Result<(), String> {
    stop_relay()
}

#[flutter_rust_bridge::frb(sync)]
pub fn relay_get_url() -> Result<String, String> {
    get_relay_url()
}

#[flutter_rust_bridge::frb(sync)]
pub fn relay_is_running() -> bool {
    is_relay_running()
}

