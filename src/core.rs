use std::{collections::VecDeque, path::Path, sync::Arc};

use pgtemp::PgTempDB;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt as _},
    net::{UnixListener, UnixStream, unix::SocketAddr},
    select,
    sync::Mutex,
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

use crate::{DatabaseConfig, Message, stats, util};

#[derive(Clone)]
pub(crate) struct Config {
    max_databases: usize,
    #[allow(dead_code)]
    prefix: String,
}

impl Config {
    pub(crate) fn new(max_databases: usize, prefix: String) -> Self {
        Self {
            max_databases,
            prefix,
        }
    }

    pub(crate) fn from_env(fallback_prefix: Option<String>) -> Self {
        let max_databases: usize = util::env_var("DATABASE_COUNT").unwrap_or(8);
        let prefix: String = util::env_var("DATABASE_PREFIX")
            .or(fallback_prefix)
            .expect("DATABASE_PREFIX must be set");
        Self::new(max_databases, prefix)
    }
}

type Databases<D> = Arc<Mutex<VecDeque<D>>>;

pub trait DbLike: std::fmt::Debug + Send + 'static {
    fn from_dbname(dbname: String) -> Self;
    fn create_config(&self) -> DatabaseConfig;
    fn fallback_prefix() -> Option<String> {
        None
    }
}

impl DbLike for DatabaseConfig {
    fn from_dbname(dbname: String) -> Self {
        DatabaseConfig::with_db(dbname)
    }

    fn create_config(&self) -> DatabaseConfig {
        self.clone()
    }
}

impl DbLike for PgTempDB {
    fn from_dbname(_: String) -> Self {
        PgTempDB::new()
    }

    fn create_config(&self) -> DatabaseConfig {
        DatabaseConfig {
            dbuser: self.db_user().to_string(),
            dbpass: self.db_pass().to_string(),
            dbport: self.db_port(),
            dbname: self.db_name().to_string(),
        }
    }

    fn fallback_prefix() -> Option<String> {
        Some("pgtemp_db_".to_string())
    }
}

async fn respond<D: DbLike>(databases: Databases<D>, mut stream: UnixStream, address: SocketAddr) {
    tokio::spawn(async move {
        debug!("New connection from {:?}", address);
        debug!("Assigning database...");
        let db = {
            loop {
                let mut dbs = databases.lock().await;
                if let Some(name) = dbs.pop_front() {
                    stats::increment_usage();
                    break name;
                }
                drop(dbs);
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                stats::TOTAL_WAIT.fetch_add(10, std::sync::atomic::Ordering::Relaxed);
            }
        };
        let instant = std::time::Instant::now();
        // Respont to the client OK:{db_name} or EMPTY:No databases available
        debug!("Assigned database: {:?}", db);
        let config: DatabaseConfig = db.create_config();
        let message = Message::Ok(config);
        let message_json = serde_json::to_string(&message).unwrap();
        if let Err(e) = stream.write_all(message_json.as_bytes()).await {
            debug!("Failed to write to stream: {}", e);
        }
        stream.flush().await.unwrap();

        let mut buffer = [0; 1024];
        if let Ok(0) = stream.read(&mut buffer).await {
            debug!("Client disconnected");
            debug!(
                "Releasing database: {:?} after {}ms usage",
                db,
                instant.elapsed().as_millis()
            );
            let mut dbs = databases.lock().await;
            dbs.push_back(db);
            stats::decrement_usage();
        }
    });
}

async fn server<D: DbLike>(
    listener: UnixListener,
    databases: Databases<D>,
    cancellation_token: CancellationToken,
    barrier: Arc<tokio::sync::Barrier>,
) {
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
}

pub(crate) async fn build_databases<D: DbLike>(config: Config) -> Databases<D> {
    let databases = Arc::new(Mutex::new(VecDeque::new()));
    let mut tasks = vec![];
    let prefix = config.prefix;
    for n in 0..config.max_databases {
        let databases = databases.clone();
        let prefix = prefix.clone();
        tasks.push(tokio::spawn(async move {
            let db = D::from_dbname(format!("{}{}", prefix, n));
            let mut dbs = databases.lock().await;
            dbs.push_back(db);
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    info!("Built {} databases", config.max_databases);
    databases
}

pub(crate) async fn start_server<D: DbLike>(
    path: &Path,
    config: Config,
) -> (tokio::task::JoinHandle<()>, CancellationToken) {
    let cancellation_token = tokio_util::sync::CancellationToken::new();
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let databases = build_databases::<D>(config).await;

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
        let listener = UnixListener::bind(path.clone()).unwrap();
        tokio::spawn(async move {
            let result = server(listener, databases, cancellation_token, barrier).await;
            info!("Shutting down server...");
            std::fs::remove_file(&path).expect("Failed to remove socket file");
            result
        })
    };
    barrier.wait().await;
    debug!("Listening on {}", path.display());
    (server, cancellation_token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers;

    #[tokio::test]
    async fn test_build_databases() {
        let config = Config::new(2, "test_db_".to_string());
        let actual = build_databases::<DatabaseConfig>(config).await;
        let actual = actual.lock().await.clone();
        let expected: VecDeque<_> = vec![
            DatabaseConfig::with_db("test_db_0".to_string()),
            DatabaseConfig::with_db("test_db_1".to_string()),
        ]
        .into();
        assert_eq!(actual, expected);
    }

    #[tokio::test]
    async fn smoke_test_start_server() {
        let path = test_helpers::temp_path();
        let config = Config::new(2, "test_db_".to_string());
        let (server, cancellation_token) = start_server::<DatabaseConfig>(&path, config).await;
        cancellation_token.cancel();
        assert!(server.await.is_ok());
    }
}
