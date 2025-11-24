use flutter_rust_bridge::frb;
use std::sync::{Arc, Mutex};
use std::thread;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::rustls::{ServerConfig, pki_types::{CertificateDer, PrivateKeyDer}};
use tokio_rustls::TlsAcceptor;
use tokio::runtime::Runtime;
use std::fs;

/// TLS proxy server that bridges WSS connections to local WS relay
/// Uses tokio-rustls which properly sends the full certificate chain
pub struct TlsProxyServer {
    tls_port: u16,
    ws_port: u16,
    server_config: Arc<ServerConfig>,
    listener: Option<tokio::task::JoinHandle<()>>,
}

impl TlsProxyServer {
    /// Create a new TLS proxy server
    /// 
    /// # Arguments
    /// * `tls_port` - Port to listen for WSS connections
    /// * `ws_port` - Port of the local WS relay to forward to
    /// * `fullchain_pem_path` - Path to fullchain.pem (contains server cert + intermediate certs)
    /// * `private_key_path` - Path to private key file
    pub fn new(
        tls_port: u16,
        ws_port: u16,
        fullchain_pem_path: String,
        private_key_path: String,
    ) -> Result<Self, String> {
        tracing::info!("📦 Loading TLS certificates from files...");
        // Load certificate chain from fullchain.pem
        let cert_chain = load_cert_chain(&fullchain_pem_path)?;
        
        // Load private key
        let private_key = load_private_key(&private_key_path)?;
        
        // Create TLS server config with full certificate chain
        // rustls 0.23 API: ServerConfig::builder() returns a builder that needs a verifier
        // We use with_no_client_auth() which provides a no-op verifier
        let server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(cert_chain, private_key)
            .map_err(|e| format!("Failed to create TLS server config: {}", e))?;
        
        Ok(Self {
            tls_port,
            ws_port,
            server_config: Arc::new(server_config),
            listener: None,
        })
    }
    
    /// Start the TLS proxy server
    pub async fn start(&mut self) -> Result<(), String> {
        if self.listener.is_some() {
            return Err("TLS proxy server is already running".to_string());
        }
        
        let addr = format!("0.0.0.0:{}", self.tls_port);
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| format!("Failed to bind to {}: {}", addr, e))?;
        
        let tls_acceptor = TlsAcceptor::from(Arc::clone(&self.server_config));
        let ws_port = self.ws_port;
        
        let handle = tokio::spawn(async move {
            tracing::info!("🚀 TLS proxy server started on {}", addr);
            
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        tracing::info!("🔗 New TLS connection from {}", addr);
                        let acceptor = tls_acceptor.clone();
                        let ws_port = ws_port;
                        
                        tokio::spawn(async move {
                            if let Err(e) = handle_tls_connection(stream, acceptor, ws_port).await {
                                tracing::error!("❌ Error handling TLS connection: {}", e);
                            }
                        });
                    }
                    Err(e) => {
                        tracing::error!("❌ Failed to accept connection: {}", e);
                    }
                }
            }
        });
        
        self.listener = Some(handle);
        Ok(())
    }
    
    /// Stop the TLS proxy server
    pub fn stop(&mut self) {
        if let Some(handle) = self.listener.take() {
            handle.abort();
            tracing::info!("🛑 TLS proxy server stopped");
        }
    }
}

/// Handle a single TLS connection by bridging it to the local WS relay
async fn handle_tls_connection(
    stream: TcpStream,
    acceptor: TlsAcceptor,
    ws_port: u16,
) -> Result<(), String> {
    // Accept TLS connection
    let tls_stream = acceptor
        .accept(stream)
        .await
        .map_err(|e| format!("TLS handshake failed: {}", e))?;
    
    tracing::info!("✅ TLS handshake successful");
    
    // Connect to local WS relay
    let ws_addr = format!("127.0.0.1:{}", ws_port);
    let ws_stream = TcpStream::connect(&ws_addr)
        .await
        .map_err(|e| format!("Failed to connect to WS relay at {}: {}", ws_addr, e))?;
    
    tracing::info!("🔌 Connected to WS relay at {}", ws_addr);
    
    // Bridge traffic between TLS client and WS backend
    let (mut tls_reader, mut tls_writer) = tokio::io::split(tls_stream);
    let (mut ws_reader, mut ws_writer) = tokio::io::split(ws_stream);
    
    // Forward TLS -> WS
    let forward_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 8192];
        loop {
            match tls_reader.read(&mut buf).await {
                Ok(0) => break, // EOF
                Ok(n) => {
                    if ws_writer.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    
    // Forward WS -> TLS
    let backward_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 8192];
        loop {
            match ws_reader.read(&mut buf).await {
                Ok(0) => break, // EOF
                Ok(n) => {
                    if tls_writer.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    
    // Wait for either direction to finish
    tokio::select! {
        _ = forward_task => {},
        _ = backward_task => {},
    }
    
    tracing::info!("🔌 Connection closed");
    Ok(())
}

/// Load certificate chain from fullchain.pem
/// This file should contain: server cert, intermediate cert 1, intermediate cert 2, etc.
fn load_cert_chain(path: &str) -> Result<Vec<CertificateDer<'static>>, String> {
    let pem_data = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read certificate file {}: {}", path, e))?;
    
    let certs = rustls_pemfile::certs(&mut pem_data.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to parse certificate PEM: {}", e))?;
    
    if certs.is_empty() {
        return Err("No certificates found in fullchain.pem".to_string());
    }
    
    tracing::info!("✅ Loaded {} certificate(s) from fullchain.pem", certs.len());
    
    Ok(certs)
}

/// Load private key from file
fn load_private_key(path: &str) -> Result<PrivateKeyDer<'static>, String> {
    let key_data = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read private key file {}: {}", path, e))?;
    
    // Try PKCS8 first (most common)
    let mut key_reader = key_data.as_bytes();
    if let Some(Ok(key)) = rustls_pemfile::pkcs8_private_keys(&mut key_reader).next() {
        return Ok(PrivateKeyDer::Pkcs8(key));
    }
    
    // Try RSA
    let mut key_reader = key_data.as_bytes();
    if let Some(Ok(key)) = rustls_pemfile::rsa_private_keys(&mut key_reader).next() {
        return Ok(PrivateKeyDer::Pkcs1(key));
    }
    
    Err("Failed to parse private key (tried PKCS8 and RSA formats)".to_string())
}

// Imports already at top of file

/// Global TLS proxy runtime and handle
struct TlsProxyRuntime {
    runtime: Arc<Runtime>,
    handle: tokio::task::JoinHandle<()>,
    thread_handle: thread::JoinHandle<()>,
}

static TLS_PROXY_RUNTIME: Mutex<Option<TlsProxyRuntime>> = Mutex::new(None);

/// Create a new runtime in a background thread
fn create_runtime_in_thread() -> Result<(Arc<Runtime>, thread::JoinHandle<()>), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    
    let thread_handle = thread::spawn(move || {
        let rt = Runtime::new().expect("Failed to create tokio runtime");
        let rt_arc = Arc::new(rt);
        tx.send(rt_arc.clone()).expect("Failed to send runtime");
        
        // Keep runtime alive by blocking on a future that never completes
        rt_arc.block_on(async {
            std::future::pending::<()>().await;
        });
    });
    
    let runtime = rx.recv()
        .map_err(|e| format!("Failed to receive runtime: {}", e))?;
    
    Ok((runtime, thread_handle))
}

/// Start TLS proxy server
/// 
/// This function starts a TLS proxy server that listens on `tls_port` for WSS connections
/// and forwards them to the local WS relay on `ws_port`.
/// 
/// The server uses `fullchain_pem` and `private_key_pem` which should contain the complete 
/// certificate chain (server cert + intermediate certs) in the correct order.
/// 
/// # Arguments
/// * `tls_port` - Port to listen for WSS connections (e.g., 28443)
/// * `ws_port` - Port of the local WS relay to forward to (e.g., 8081)
/// * `fullchain_pem` - Full certificate chain in PEM format (as string)
/// * `private_key_pem` - Private key in PEM format (as string)
#[frb(sync)]
pub fn tls_proxy_start(
    tls_port: u16,
    ws_port: u16,
    fullchain_pem: String,
    private_key_pem: String,
) -> Result<(), String> {
    // Stop existing server if any
    tls_proxy_stop()?;
    
    // Write certificates to temporary files
    // Note: In production, you might want to use a more secure location
    let temp_dir = std::env::temp_dir();
    let fullchain_path = temp_dir.join("localrelay_fullchain.pem");
    let key_path = temp_dir.join("localrelay_privatekey.pem");
    
    std::fs::write(&fullchain_path, fullchain_pem)
        .map_err(|e| format!("Failed to write fullchain.pem: {}", e))?;
    std::fs::write(&key_path, private_key_pem)
        .map_err(|e| format!("Failed to write private key: {}", e))?;
    
    // Create new server
    let mut server = TlsProxyServer::new(
        tls_port,
        ws_port,
        fullchain_path.to_string_lossy().to_string(),
        key_path.to_string_lossy().to_string(),
    )?;
    
    // Create runtime in background thread
    let (runtime, thread_handle) = create_runtime_in_thread()?;
    
    // Start server in the runtime
    let handle = runtime.spawn(async move {
        if let Err(e) = server.start().await {
            tracing::error!("Failed to start TLS proxy server: {}", e);
        } else {
            // Keep the server running
            // The server's listener loop will keep running
            std::future::pending::<()>().await;
        }
    });
    
    // Store runtime and handle
    let mut runtime_guard = TLS_PROXY_RUNTIME.lock()
        .map_err(|e| format!("Failed to lock TLS proxy runtime: {}", e))?;
    *runtime_guard = Some(TlsProxyRuntime {
        runtime,
        handle,
        thread_handle,
    });
    
    tracing::info!("✅ TLS proxy server started on port {}", tls_port);
    Ok(())
}

/// Stop TLS proxy server
#[frb(sync)]
pub fn tls_proxy_stop() -> Result<(), String> {
    let mut runtime_guard = TLS_PROXY_RUNTIME.lock()
        .map_err(|e| format!("Failed to lock TLS proxy runtime: {}", e))?;
    
    if let Some(runtime) = runtime_guard.take() {
        runtime.handle.abort();
        // Note: We can't easily stop the background thread, but aborting the handle
        // will stop the server. The thread will continue running but that's acceptable.
        tracing::info!("🛑 TLS proxy server stopped");
    }
    
    Ok(())
}

