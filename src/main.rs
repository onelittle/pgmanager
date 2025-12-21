use std::{
    collections::VecDeque,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, atomic::AtomicUsize},
};

use clap::{Parser, Subcommand, command};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt as _},
    net::UnixListener,
    select,
    sync::Mutex,
};
use tracing::{debug, info, warn};

static USAGE: AtomicUsize = AtomicUsize::new(0);
static PEAK_USAGE: AtomicUsize = AtomicUsize::new(0);
static TOTAL_WAIT: AtomicUsize = AtomicUsize::new(0);

fn increment_usage() {
    let current = USAGE.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    let peak = PEAK_USAGE.load(std::sync::atomic::Ordering::Relaxed);
    if current > peak {
        debug!("Peak usage: {}", current);
        PEAK_USAGE.store(current, std::sync::atomic::Ordering::Relaxed);
    }
}

fn decrement_usage() -> usize {
    USAGE.fetch_sub(1, std::sync::atomic::Ordering::Relaxed)
}

fn env_var<T: FromStr>(key: &str) -> Option<T> {
    std::env::var(format!("PGM_{}", key))
        .or_else(|_| std::env::var(key))
        .map_err(|e| {
            warn!("Environment variable {} not found: {}", key, e);
            warn!("{}", e);
        })
        .ok()
        .and_then(|v| v.parse().ok())
}

fn serve(
    path: PathBuf,
    cancellation_token: tokio_util::sync::CancellationToken,
    barrier: Option<Arc<tokio::sync::Barrier>>,
) -> tokio::task::JoinHandle<()> {
    let mut databases: VecDeque<String> = VecDeque::new();
    let max_count: usize = env_var("DATABASE_COUNT").unwrap_or(8);
    let db_prefix: String = env_var("DATABASE_PREFIX").expect("DATABASE_PREFIX must be set");
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
    let listener = UnixListener::bind(&path).unwrap();
    tokio::spawn(async move {
        debug!("Listening on {}", path.display());
        if let Some(barrier) = barrier {
            barrier.wait().await;
        }
        loop {
            select! {
                _ = cancellation_token.cancelled() => {
                    break;
                },
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((mut stream, addr)) => {
                            let databases = databases.clone();
                            tokio::spawn(async move {
                                debug!("New connection from {:?}", addr);
                                debug!("Assigning database...");
                                let name = {
                                    loop {
                                        let mut dbs = databases.lock().await;
                                        if let Some(name) = dbs.pop_front() {
                                            increment_usage();
                                            break name.clone();
                                        }
                                        drop(dbs);
                                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                                        TOTAL_WAIT.fetch_add(10, std::sync::atomic::Ordering::Relaxed);
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
                                    decrement_usage();
                                }
                            });
                        }
                        Err(_) => { /* connection failed */ }
                    }
                }
            }
        }

        info!("Shutting down server...");
        std::fs::remove_file(&path).unwrap();
    })
}

#[derive(Parser)]
struct Cli {
    /// Path to the Unix domain socket
    #[clap(short, long, default_value = "tmp/test_manager.sock")]
    socket: String,
    /// Enable verbose logging
    #[clap(short, long, default_value_t = false)]
    verbose: bool,
    // Subcommand to wrap another command
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[command()]
    Serve,
    #[command()]
    Wrap {
        #[arg(last = true)]
        command: Vec<String>,
    },
}

#[tokio::main]
async fn main() {
    let args = Cli::parse();

    tracing_subscriber::fmt()
        .with_max_level(if args.verbose {
            tracing::Level::DEBUG
        } else {
            tracing::Level::INFO
        })
        .init();

    let path = if args.socket.starts_with("/") {
        PathBuf::from(args.socket)
    } else {
        std::env::current_dir().unwrap().join(args.socket)
    };

    match args.command {
        Commands::Serve => {
            let cancellation_token = tokio_util::sync::CancellationToken::new();
            let server = tokio::spawn(serve(path, cancellation_token.clone(), None));
            match tokio::signal::ctrl_c().await {
                Ok(()) => {
                    info!("Received shutdown signal, shutting down...");
                    cancellation_token.cancel();
                    server.await.unwrap().unwrap();
                    info!(
                        "Peak usage: {}",
                        PEAK_USAGE.load(std::sync::atomic::Ordering::Relaxed)
                    );
                    info!(
                        "Total wait time: {}ms",
                        TOTAL_WAIT.load(std::sync::atomic::Ordering::Relaxed)
                    );
                }
                Err(err) => {
                    info!("Unable to listen for shutdown signal: {}", err);
                }
            }
        }
        Commands::Wrap { command } => {
            let barrier = Arc::new(tokio::sync::Barrier::new(2));
            let cancellation_token = tokio_util::sync::CancellationToken::new();
            let server = tokio::spawn(serve(
                path.clone(),
                cancellation_token.clone(),
                Some(barrier.clone()),
            ));
            barrier.wait().await;

            // Run the command as passed and send PGMANAGER_SOCKET env var
            let command = command.join(" ");
            let mut cmd = tokio::process::Command::new("sh");
            cmd.arg("-c").arg(&command);
            cmd.env("PGMANAGER_SOCKET", path.to_str().unwrap());
            let status = cmd.status().await.unwrap();
            cancellation_token.cancel();
            server.await.unwrap().unwrap();
            std::process::exit(status.code().unwrap_or(1));
        }
    }
}
