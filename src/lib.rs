mod stats;
mod util;

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::Arc,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt as _},
    net::{UnixListener, UnixStream, unix::SocketAddr},
    select,
    sync::Mutex,
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

type Databases = Arc<Mutex<VecDeque<String>>>;

async fn respond(databases: Databases, mut stream: UnixStream, address: SocketAddr) {
    tokio::spawn(async move {
        debug!("New connection from {:?}", address);
        debug!("Assigning database...");
        let name = {
            loop {
                let mut dbs = databases.lock().await;
                if let Some(name) = dbs.pop_front() {
                    stats::increment_usage();
                    break name.clone();
                }
                drop(dbs);
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                stats::TOTAL_WAIT.fetch_add(10, std::sync::atomic::Ordering::Relaxed);
            }
        };
        let instant = std::time::Instant::now();
        // Respont to the client OK:{db_name} or EMPTY:No databases available
        debug!("Assigned database: {:?}", name);
        if let Err(e) = stream.write_all(format!("OK:{}", name).as_bytes()).await {
            debug!("Failed to write to stream: {}", e);
        }
        stream.flush().await.unwrap();

        let mut buffer = [0; 1024];
        if let Ok(0) = stream.read(&mut buffer).await {
            debug!("Client disconnected");
            debug!(
                "Releasing database: {} after {}ms usage",
                name,
                instant.elapsed().as_millis()
            );
            let mut dbs = databases.lock().await;
            dbs.push_back(name);
            stats::decrement_usage();
        }
    });
}

async fn server(
    path: PathBuf,
    databases: Databases,
    cancellation_token: CancellationToken,
    barrier: Arc<tokio::sync::Barrier>,
) {
    let listener = UnixListener::bind(path.clone()).unwrap();
    debug!("Listening on {}", path.display());
    barrier.wait().await;
    loop {
        select! {
            _ = cancellation_token.cancelled() => {
                break;
            },
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, addr)) => {
                        let databases = databases.clone();
                        respond(databases, stream, addr).await;
                    }
                    Err(_) => { /* connection failed */ }
                }
            }
        }
    }

    info!("Shutting down server...");
    std::fs::remove_file(&path).unwrap();
}

async fn start_server(path: &Path) -> (tokio::task::JoinHandle<()>, CancellationToken) {
    let cancellation_token = tokio_util::sync::CancellationToken::new();
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let max_count: usize = util::env_var("DATABASE_COUNT").unwrap_or(8);
    let db_prefix: String = util::env_var("DATABASE_PREFIX").expect("DATABASE_PREFIX must be set");
    let mut databases: VecDeque<String> = VecDeque::new();
    for n in 0..max_count {
        databases.push_back(format!("{}{}", db_prefix, n));
    }

    let databases = Arc::new(Mutex::new(databases));

    if path.is_dir() {
        panic!("Socket path cannot be a directory");
    }
    let parent_dir = path.parent().expect("Socket needs to be in a directory");
    if !parent_dir.exists() {
        std::fs::create_dir_all(parent_dir).unwrap();
    }
    let server = {
        let path = path.to_path_buf();
        let cancellation_token = cancellation_token.clone();
        let barrier = barrier.clone();
        tokio::spawn(server(path, databases, cancellation_token, barrier))
    };
    barrier.wait().await;
    (server, cancellation_token)
}

pub async fn serve(path: &Path) {
    let (server, cancellation_token) = start_server(path).await;

    match tokio::signal::ctrl_c().await {
        Ok(()) => {
            info!("Received shutdown signal, shutting down...");
            cancellation_token.cancel();
            server.await.unwrap();
            stats::log_usage();
        }
        Err(err) => {
            info!("Unable to listen for shutdown signal: {}", err);
        }
    }
}

pub async fn wrap(path: &Path, command: Vec<String>) {
    let (server, cancellation_token) = start_server(path).await;

    // Run the command as passed and send PGMANAGER_SOCKET env var
    let command = command.join(" ");
    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c").arg(&command);
    cmd.env("PGMANAGER_SOCKET", path.to_str().unwrap());
    let status = cmd.status().await.unwrap();
    cancellation_token.cancel();
    server.await.unwrap();
    std::process::exit(status.code().unwrap_or(1));
}

pub struct DatabaseGuard {
    pub name: String,
    _stream: UnixStream,
}

pub async fn get_database() -> Result<DatabaseGuard, Box<dyn std::error::Error>> {
    let path = std::env::var("PGMANAGER_SOCKET").expect("PGMANAGER_SOCKET must be set");
    let mut stream = tokio::net::UnixStream::connect(path)
        .await
        .expect("Failed to connect to test manager socket");
    let mut buffer = [0; 1024];
    let read = stream
        .read(&mut buffer)
        .await
        .expect("Failed to read from test manager socket");
    if read == 0 {
        panic!("Test manager socket closed unexpectedly");
    }
    let response = String::from_utf8_lossy(&buffer);
    if response.starts_with("OK:") {
        let db_name = response.strip_prefix("OK:").unwrap().trim().to_string();
        // Remove embedded null characters
        let db_name = db_name.replace('\0', "");

        eprintln!("Using test database: {}", db_name);
        return Ok(DatabaseGuard {
            name: db_name,
            _stream: stream,
        });
    }

    if response.starts_with("EMPTY:") {
        panic!(
            "No databases available: {}",
            response.strip_prefix("ERROR:").unwrap().trim()
        );
    }

    panic!("Unexpected response from test manager: {}", response);
}
