use nostr_relay_builder::{LocalRelay, builder::RateLimit};
use nostr_ndb::NdbDatabase;
use std::sync::{Arc, Mutex};
use std::net::IpAddr;
use std::path::PathBuf;
use std::fs::OpenOptions;
use tokio::runtime::Runtime;
use serde::{Serialize, Deserialize};
use nostr_database::prelude::Filter;
use nostr_database::NostrDatabase;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Layer;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::FormatFields;
use tracing_appender::non_blocking::WorkerGuard;

/// Setup log file and initialize tracing
/// Returns the log file path string
fn setup_log_file(db_path: &str) -> Result<String, String> {
    // Setup log file path (in same directory as database)
    let db_path_buf = PathBuf::from(db_path);
    let log_dir = db_path_buf.parent()
        .ok_or_else(|| "Invalid database path".to_string())?;
    let log_file_path = log_dir.join("relay.log");
    let log_file_path_str = log_file_path.to_string_lossy().to_string();
    
    // Initialize tracing with file and console output
    {
        let mut log_path_guard = LOG_FILE_PATH.lock()
            .map_err(|e| format!("Failed to lock log file path: {}", e))?;
        *log_path_guard = Some(log_file_path_str.clone());
    }
    
    // Limit log file to 200 lines if it exists
    let _ = limit_log_file_lines(&log_file_path, 200);
    
    // Delete any old rotated log files (cleanup from previous version)
    let log_dir = log_file_path.parent()
        .ok_or_else(|| "Invalid log file path".to_string())?;
    let log_file_name = log_file_path.file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "Invalid log file name".to_string())?;
    for i in 1..=10 {
        let rotated_file = log_dir.join(format!("{}.{}", log_file_name, i));
        if rotated_file.exists() {
            let _ = std::fs::remove_file(&rotated_file);
        }
    }
    
    // Create log file writer
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file_path)
        .map_err(|e| format!("Failed to open log file: {}", e))?;
    
    // Create non-blocking writer for file logging
    let (non_blocking, guard) = tracing_appender::non_blocking(log_file);
    
    // Store guard to keep it alive
    {
        let mut guard_storage = LOG_GUARD.lock()
            .map_err(|e| format!("Failed to lock log guard: {}", e))?;
        *guard_storage = Some(guard);
    }
    
    // Custom log formatter that only logs events, queries, and errors
    struct EventOnlyFormatter;
    
    impl<S, N> tracing_subscriber::fmt::FormatEvent<S, N> for EventOnlyFormatter
    where
        S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
        N: for<'a> tracing_subscriber::fmt::FormatFields<'a> + 'static,
    {
        fn format_event(
            &self,
            ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
            mut writer: Writer<'_>,
            event: &tracing::Event<'_>,
        ) -> std::fmt::Result {
            use tracing::field::Visit;
            use std::fmt::Write;
            
            // Collect the message - try to get all fields
            let mut message = String::new();
            struct MessageVisitor<'a>(&'a mut String);
            impl<'a> Visit for MessageVisitor<'a> {
                fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                    if field.name() == "message" {
                        write!(self.0, "{:?}", value).ok();
                    } else if self.0.is_empty() {
                        // If no message field, use the first field as message
                        write!(self.0, "{}={:?}", field.name(), value).ok();
                    }
                }
            }
            event.record(&mut MessageVisitor(&mut message));
            
            // If message is still empty, format all fields
            if message.is_empty() {
                ctx.format_fields(writer.by_ref(), event)?;
                writeln!(writer)?;
                return Ok(());
            }
            
            // Check if this log should be recorded
            let level = *event.metadata().level();
            
            // Only record ERROR, WARN, and INFO levels (no DEBUG or TRACE logs)
            // DEBUG and TRACE logs are filtered out completely
            let should_log = level <= tracing::Level::INFO;
            
            if !should_log {
                return Ok(());
            }
            
            // Format timestamp (simple format: HH:MM:SS.mmm)
            use std::time::SystemTime;
            if let Ok(elapsed) = SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                let total_secs = elapsed.as_secs();
                let millis = elapsed.subsec_millis();
                // Calculate time of day (seconds since midnight)
                let time_of_day = total_secs % 86400;
                let hours = time_of_day / 3600;
                let minutes = (time_of_day % 3600) / 60;
                let seconds = time_of_day % 60;
                write!(writer, "{:02}:{:02}:{:02}.{:03} ", hours, minutes, seconds, millis)?;
            }
            
            // Format level
            match level {
                tracing::Level::ERROR => write!(writer, "[ERROR] ")?,
                tracing::Level::WARN => write!(writer, "[WARN] ")?,
                tracing::Level::INFO => write!(writer, "[INFO] ")?,
                tracing::Level::DEBUG => write!(writer, "[DEBUG] ")?,
                tracing::Level::TRACE => write!(writer, "[TRACE] ")?,
            }
            
            // Write the message
            if !message.is_empty() {
                write!(writer, "{}", message)?;
            } else {
                ctx.format_fields(writer.by_ref(), event)?;
            }
            
            writeln!(writer)?;
            Ok(())
        }
    }
    
    // Initialize tracing subscriber with custom formatter for file output
    let _ = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_target(false)
                .with_ansi(false)
                .event_format(EventOnlyFormatter)
                .with_filter(LevelFilter::DEBUG)
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(false)
                .with_filter(LevelFilter::INFO)
        )
        .try_init();
    
    Ok(log_file_path_str)
}

/// Limit log file to max_lines by keeping only the last N lines
fn limit_log_file_lines(log_file_path: &PathBuf, max_lines: usize) -> Result<(), String> {
    if !log_file_path.exists() {
        return Ok(());
    }
    
    // Read all lines
    let content = std::fs::read_to_string(log_file_path)
        .map_err(|e| format!("Failed to read log file: {}", e))?;
    
    let lines: Vec<&str> = content.lines().collect();
    
    // If file has more than max_lines, keep only the last max_lines
    if lines.len() > max_lines {
        let start = lines.len() - max_lines;
        let truncated_content = lines[start..].join("\n");
        
        std::fs::write(log_file_path, truncated_content)
            .map_err(|e| format!("Failed to write truncated log file: {}", e))?;
    }
    
    Ok(())
}

/// Clear log file content
fn clear_log_file() -> Result<(), String> {
    let log_path_guard = LOG_FILE_PATH.lock()
        .map_err(|e| format!("Failed to lock log file path: {}", e))?;
    
    let log_path = log_path_guard.as_ref()
        .ok_or_else(|| "Log file path not set".to_string())?;
    
    let log_file_path = PathBuf::from(log_path);
    
    // Clear the log file by writing empty content
    std::fs::write(&log_file_path, "")
        .map_err(|e| format!("Failed to clear log file: {}", e))?;

    Ok(())
}

// Global relay instance
static RELAY_INSTANCE: Mutex<Option<Arc<LocalRelay>>> = Mutex::new(None);
static RELAY_CLIENT_URL: Mutex<Option<String>> = Mutex::new(None);
static RELAY_DATABASE: Mutex<Option<Arc<NdbDatabase>>> = Mutex::new(None);
static RUNTIME: Mutex<Option<Arc<Runtime>>> = Mutex::new(None);
static LOG_FILE_PATH: Mutex<Option<String>> = Mutex::new(None);
static LOG_GUARD: Mutex<Option<WorkerGuard>> = Mutex::new(None);
static RELAY_CONFIG: Mutex<Option<(String, u16, String)>> = Mutex::new(None);

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

/// Start the relay
/// 
/// # Arguments
/// * `host` - IP address to bind (e.g. "127.0.0.1" or "0.0.0.0")
/// * `port` - Port number (e.g. 8081)
/// * `db_path` - Database path (reserved for future persistent storage)
pub async fn start_relay(host: String, port: u16, db_path: String) -> Result<String, String> {
    // Setup log file and initialize tracing
    let log_file_path_str = setup_log_file(&db_path)?;
    
    tracing::info!("🚀 Starting relay...");
    
    // Clear client URL
    {
        let mut url_guard = RELAY_CLIENT_URL.lock()
            .map_err(|e| format!("Failed to lock client URL: {}", e))?;
        *url_guard = None;
    }
    
    // Create or reopen database
    let database_arc = {
    // Create parent directory if it doesn't exist
    let db_path_buf = PathBuf::from(&db_path);
    if let Some(parent) = db_path_buf.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create database directory: {}", e))?;
    }
    
        let mut db_guard = RELAY_DATABASE.lock()
            .map_err(|e| format!("Failed to lock database: {}", e))?;
        
        tracing::info!("Opening database connection...");
        
        let db = NdbDatabase::open(&db_path)
            .map_err(|e| format!("Failed to open NDB database: {}", e))?;
        let db_arc = Arc::new(db);
        *db_guard = Some(db_arc.clone());
        db_arc
    };
    
    // Parse IP address
    let addr: IpAddr = host.parse()
        .map_err(|e| format!("Invalid IP address '{}': {}", host, e))?;
    
    // Build new relay with fresh database
    // Configure rate limit: more lenient for local relay
    let rate_limit = RateLimit {
        max_reqs: 1000,          // Max active subscriptions per connection (default: 500)
        notes_per_minute: 600,   // Max events per minute per connection (default: 60)
    };
    
    let relay = LocalRelay::builder()
        .addr(addr)
        .port(port)
        .database(database_arc)
        .rate_limit(rate_limit)
        .build();
    
    // Store relay instance first (before any await that might need Send)
    let relay_arc = Arc::new(relay);
    {
        let mut relay_guard = RELAY_INSTANCE.lock()
            .map_err(|e| format!("Failed to lock relay instance: {}", e))?;
        *relay_guard = Some(relay_arc.clone());
    }
    
    // Store relay configuration for manual restart
    {
        let mut config_guard = RELAY_CONFIG.lock()
            .map_err(|e| format!("Failed to lock relay config: {}", e))?;
        *config_guard = Some((host.clone(), port, db_path.clone()));
    }
    
    // Get URL after storing relay (all MutexGuard are dropped)
    let relay_url = relay_arc.url().await;
    let url = relay_url.to_string();
    
    // Fix URL: Replace 0.0.0.0 with 127.0.0.1 for client connections
    let client_url = if addr.to_string() == "0.0.0.0" {
        format!("ws://127.0.0.1:{}", port)
    } else {
        url.clone()
    };
    
    // Store client URL
    {
        let mut url_guard = RELAY_CLIENT_URL.lock()
            .map_err(|e| format!("Failed to lock client URL: {}", e))?;
        *url_guard = Some(client_url.clone());
    }
    
    tracing::info!("✅ Relay started on {}", client_url);
    tracing::info!("Log file: {}", log_file_path_str);
    
    // Use a channel to wait for relay to actually start listening
    let (tx, mut rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    
    // Spawn relay run task in background
    tokio::spawn(async move {
        // Wait a brief moment for relay to start listening
        // This gives the relay time to bind to the port and start accepting connections
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        
        // Signal that relay startup is complete
        let _ = tx.send(Ok(()));
        
        // Get current relay instance
        let current_relay = {
            let relay_guard = RELAY_INSTANCE.lock().ok();
            if let Some(guard) = relay_guard {
                guard.as_ref().cloned()
            } else {
                None
            }
        };
        
        // Check if relay should still be running
        let relay = match current_relay {
            Some(r) => r,
            None => {
                tracing::info!("Relay stopped by user, exiting run task");
                return;
            }
        };
        
        // Run relay (this will block until relay stops)
        let result = relay.run().await;
        
        // Log result but don't handle errors - let relay continue running if possible
        match result {
            Ok(_) => {
                tracing::info!("Relay stopped normally");
            }
            Err(e) => {
                let error_string = e.to_string();
                tracing::error!("Relay error: {}", error_string);
                // Don't exit on error - some errors may not affect relay operation
            }
        }
    });
    
    // Wait for relay to be ready (with timeout)
    tokio::select! {
        result = &mut rx => {
            match result {
                Ok(Ok(_)) => {
                    tracing::info!("Relay startup confirmed, ready to accept connections");
                }
                Ok(Err(e)) => {
                    tracing::warn!("Relay startup reported error: {}", e);
                }
                Err(e) => {
                    tracing::warn!("Failed to wait for relay startup signal: {}", e);
                }
            }
        }
        _ = tokio::time::sleep(tokio::time::Duration::from_millis(500)) => {
            tracing::warn!("Timeout waiting for relay startup confirmation, continuing anyway");
        }
    }
    
    Ok(client_url)
}

/// Stop the relay and clean up all global data
pub async fn stop_relay() -> Result<(), String> {
    // Take relay instance and drop the guard before any await
    let relay = {
    let mut relay_guard = RELAY_INSTANCE.lock()
        .map_err(|e| format!("Failed to lock relay instance: {}", e))?;
        relay_guard.take()
    };
    
    if let Some(relay) = relay {
        // Shutdown the relay instance
        relay.shutdown();
        // relay instance will be dropped here when it goes out of scope
        
        // Clear client URL
        {
        if let Ok(mut url_guard) = RELAY_CLIENT_URL.lock() {
            *url_guard = None;
            }
        }
        
        // Clear relay config
        {
            if let Ok(mut config_guard) = RELAY_CONFIG.lock() {
                *config_guard = None;
            }
        }
        
        // Clear database - explicitly drop to release file locks
        {
            if let Ok(mut db_guard) = RELAY_DATABASE.lock() {
                if let Some(db) = db_guard.take() {
                    // Explicitly drop the Arc and wait for underlying resources to be freed
                    drop(db);
                    tracing::info!("Database connection closed");
                }
            }
        }
        
        // Clear log guard
        {
        if let Ok(mut guard_storage) = LOG_GUARD.lock() {
            *guard_storage = None;
            }
        }
        
        tracing::info!("Relay stopped");
        
        // Wait longer to ensure all file handles and locks are released by the OS
        // This is critical for LMDB database files (data.mdb, lock.mdb)
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        
        Ok(())
    } else {
        Err("Relay is not running".to_string())
    }
}

/// Restart the relay by stopping (cleaning up all global data) and then starting (reinitializing)
pub async fn restart_relay() -> Result<String, String> {
    // Step 1: Get relay configuration before stopping
    let (host, port, db_path) = {
        let config_guard = RELAY_CONFIG.lock()
            .map_err(|e| format!("Failed to lock relay config: {}", e))?;
        
        config_guard.as_ref()
            .cloned()
            .ok_or_else(|| "Relay is not running, cannot restart".to_string())?
    };
    
    // Step 2: Stop relay (cleans up all global data)
    tracing::info!("Stopping relay for restart...");
    stop_relay().await?;
    
    // Longer delay to ensure complete resource cleanup
    // This gives the OS time to release database file locks (LMDB lock.mdb)
    // and other file handles that might cause IO errors on restart
    tracing::info!("Waiting for complete resource cleanup...");
    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    
    // Step 3: Start relay (reinitializes all global data and starts monitoring)
    tracing::info!("Starting relay after restart...");
    start_relay(host, port, db_path).await
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

/// Relay statistics (event-focused)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayStats {
    pub total_events: u64,
}

/// Get relay statistics
pub async fn get_relay_stats(db_path: String) -> Result<RelayStats, String> {
    // Get database reference or open it
    let database = {
        let db_guard = RELAY_DATABASE.lock()
            .map_err(|e| format!("Failed to lock database: {}", e))?;
        
        if let Some(db) = db_guard.as_ref() {
            db.clone()
        } else {
            // Database not in memory, open it
            let db = NdbDatabase::open(&db_path)
                .map_err(|e| format!("Failed to open database: {}", e))?;
            Arc::new(db)
        }
    };
    
    // Query statistics directly in async context (no need for runtime creation)
    let total_events = database.count(Filter::new()).await
        .map_err(|e| format!("Failed to count events: {}", e))? as u64;
    
    Ok(RelayStats { total_events })
}

// FFI-compatible functions using flutter_rust_bridge
pub async fn relay_start(host: String, port: u16, db_path: String) -> Result<String, String> {
    start_relay(host, port, db_path).await
}

pub async fn relay_stop() -> Result<(), String> {
    stop_relay().await
}

pub async fn relay_restart() -> Result<String, String> {
    restart_relay().await
}

pub async fn relay_get_url() -> Result<String, String> {
    Ok(get_relay_url()?)
}

pub async fn relay_is_running() -> bool {
    is_relay_running()
}

/// Get log file path
pub fn get_log_file_path() -> Result<String, String> {
    let log_path_guard = LOG_FILE_PATH.lock()
        .map_err(|e| format!("Failed to lock log file path: {}", e))?;
    
    if let Some(path) = log_path_guard.as_ref() {
        Ok(path.clone())
    } else {
        Err("Log file path not set".to_string())
    }
}

/// Read log file content (last N lines)
/// Only reads from the single log file (no rotation)
/// Automatically truncates file to 200 lines if it exceeds the limit
pub fn read_log_file(max_lines: Option<u32>) -> Result<String, String> {
    let log_path_guard = LOG_FILE_PATH.lock()
        .map_err(|e| format!("Failed to lock log file path: {}", e))?;
    
    let log_path = log_path_guard.as_ref()
        .ok_or_else(|| "Log file path not set".to_string())?;
    
    let log_file_path = PathBuf::from(log_path);
    
    // Read only the current log file
    if !log_file_path.exists() {
        return Ok("Log file does not exist yet.".to_string());
    }
    
    let content = std::fs::read_to_string(&log_file_path)
        .map_err(|e| format!("Failed to read log file: {}", e))?;
    
    if content.is_empty() {
        return Ok("Log file is empty.".to_string());
    }
    
    let lines: Vec<&str> = content.lines().collect();
    
    // Always limit file to 200 lines maximum
    if lines.len() > 200 {
        let start = lines.len() - 200;
        let truncated_content = lines[start..].join("\n");
        // Truncate the file to keep only last 200 lines
        std::fs::write(&log_file_path, &truncated_content)
            .map_err(|e| format!("Failed to truncate log file: {}", e))?;
        
        // Return requested number of lines (or all if not specified)
        let max = max_lines.map(|n| n as usize).unwrap_or(200).min(200);
        if max < 200 {
            let start_idx = 200 - max;
            Ok(truncated_content.lines().skip(start_idx).collect::<Vec<_>>().join("\n"))
        } else {
            Ok(truncated_content)
        }
    } else {
        // File is within limit, return requested number of lines
        let max = max_lines.map(|n| n as usize).unwrap_or(200).min(200);
        if lines.len() > max {
            let start = lines.len() - max;
            Ok(lines[start..].join("\n"))
        } else {
            Ok(content)
        }
    }
}

pub async fn relay_get_log_file_path() -> Result<String, String> {
    Ok(get_log_file_path()?)
}

pub async fn relay_read_log_file(max_lines: Option<u32>) -> Result<String, String> {
    Ok(read_log_file(max_lines)?)
}

pub async fn relay_clear_log_file() -> Result<(), String> {
    Ok(clear_log_file()?)
}

