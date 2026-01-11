pub mod commands;
mod core;
mod stats;
mod util;

pub use core::DbLike;
use serde::{Deserialize, Serialize};
use std::{fmt::Display, ops::Deref};
use tokio::{io::AsyncReadExt, net::UnixStream};

pub const DEFAULT_SOCKET_PATH: &str = "tmp/pgmanager.sock";

#[derive(Serialize, Deserialize)]
#[non_exhaustive]
enum Message {
    Ok(DatabaseConfig),
    Empty(String),
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct DatabaseConfig {
    dbuser: String,
    dbpass: String,
    dbport: u16,
    dbname: String,
}

impl DatabaseConfig {
    /// Returns the database username used when connecting to the postgres server.
    pub fn db_user(&self) -> &str {
        &self.dbuser
    }

    /// Returns the database password used when connecting to the postgres server.
    pub fn db_pass(&self) -> &str {
        &self.dbpass
    }

    /// Returns the port the postgres server is running on.
    pub fn db_port(&self) -> u16 {
        self.dbport
    }

    /// Returns the the name of the database created.
    pub fn db_name(&self) -> &str {
        &self.dbname
    }

    /// Returns a connection string that can be passed to a libpq connection function.
    ///
    /// Example output:
    /// `host=localhost port=15432 user=pgtemp password=pgtemppw-9485 dbname=pgtempdb-324`
    pub fn connection_string(&self) -> String {
        format!(
            "host=localhost port={} user={} password={} dbname={}",
            self.db_port(),
            self.db_user(),
            self.db_pass(),
            self.db_name()
        )
    }

    /// Returns a generic connection URI that can be passed to most SQL libraries' connect
    /// methods.
    ///
    /// Example output:
    /// `postgresql://pgmanager:pgmanagerpw-9485@localhost:15432/pgmanagerdb-324`
    pub fn connection_uri(&self) -> String {
        format!(
            "postgresql://{}:{}@localhost:{}/{}",
            self.db_user(),
            self.db_pass(),
            self.db_port(),
            self.db_name()
        )
    }

    pub(crate) fn with_db(dbname: String) -> Self {
        let dbuser = std::env::var("PGUSER")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or("postgres".to_string());
        let dbpass = std::env::var("PGPASSWORD").ok().unwrap_or("".to_string());
        let dbport = std::env::var("PGPORT")
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(5432);
        Self {
            dbuser,
            dbpass,
            dbport,
            dbname,
        }
    }
}

pub struct DatabaseGuard {
    config: DatabaseConfig,
    _stream: UnixStream,
}

impl Deref for DatabaseGuard {
    type Target = DatabaseConfig;

    fn deref(&self) -> &Self::Target {
        &self.config
    }
}

impl Display for DatabaseGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.connection_string())
    }
}

impl From<&DatabaseGuard> for String {
    fn from(value: &DatabaseGuard) -> String {
        value.to_string()
    }
}

pub async fn get_database() -> DatabaseGuard {
    let path = util::env_var_with_fallback("PGM_SOCKET", "PGMANAGER_SOCKET")
        .unwrap_or_else(|| DEFAULT_SOCKET_PATH.to_string());

    let stream = UnixStream::connect(path)
        .await
        .expect("Failed to connect to test manager socket");
    get_database_from_stream(stream).await
}

async fn get_database_from_stream(mut stream: UnixStream) -> DatabaseGuard {
    let mut buffer = [b' '; 1024];
    let read = stream
        .read(&mut buffer)
        .await
        .expect("Failed to read from test manager socket");
    if read == 0 {
        panic!("Test manager socket closed unexpectedly");
    }
    let response = String::from_utf8_lossy(&buffer);
    let message: Message =
        serde_json::from_str(&response).expect("Failed to read config from test manager");
    match message {
        Message::Ok(config) => {
            eprintln!("Using test database: {}", config.db_name());
            DatabaseGuard {
                config,
                _stream: stream,
            }
        }
        Message::Empty(message) => {
            panic!("No databases available: {message}");
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use pgtemp::PgTempDB;
    use std::env;

    use super::*;

    #[tokio::test]
    async fn test_get_database() {
        let path = test_helpers::temp_path();
        let (server, cancellation_token) =
            test_helpers::temp_server::<DatabaseConfig>(&path, None).await;

        let stream = test_helpers::temp_client(&path).await;
        let db_guard_a = get_database_from_stream(stream).await;
        let stream = test_helpers::temp_client(&path).await;
        let db_guard_b = get_database_from_stream(stream).await;

        assert!(db_guard_a.config.db_name().starts_with("test_db_"));
        assert!(db_guard_b.config.db_name().starts_with("test_db_"));
        assert_ne!(db_guard_a.config, db_guard_b.config);
        cancellation_token.cancel();
        server.await.expect("Server task failed");
    }

    #[tokio::test]
    async fn test_get_database_pgtemp() {
        let path = test_helpers::temp_path();
        let (server, cancellation_token) = test_helpers::temp_server::<PgTempDB>(&path, None).await;

        let stream = test_helpers::temp_client(&path).await;
        let db_guard_a = get_database_from_stream(stream).await;
        let stream = test_helpers::temp_client(&path).await;
        let db_guard_b = get_database_from_stream(stream).await;

        assert_ne!(db_guard_a.config, db_guard_b.config);
        cancellation_token.cancel();
        server.await.expect("Server task failed");
    }

    #[tokio::test]
    async fn test_formatting() {
        unsafe {
            env::set_var("PGUSER", "postgres");
        }
        let path = test_helpers::temp_path();
        let config = Some(core::Config::new(1, "test_db".into()));
        let (server, cancellation_token) =
            test_helpers::temp_server::<DatabaseConfig>(&path, config).await;

        let stream = test_helpers::temp_client(&path).await;
        let db_name = get_database_from_stream(stream).await;
        let message = format!("A database is available at {}", db_name);

        assert_eq!(
            db_name.to_string(),
            "host=localhost port=5432 user=postgres password= dbname=test_db0".to_string()
        );
        assert_eq!(
            message,
            "A database is available at host=localhost port=5432 user=postgres password= dbname=test_db0"
        );
        cancellation_token.cancel();
        server.await.expect("Server task failed");
    }
}

#[cfg(test)]
pub(crate) mod test_helpers {
    use tokio::{net::UnixStream, task::JoinHandle};
    use tokio_util::sync::CancellationToken;

    use crate::core::{self, DbLike};

    pub fn temp_path() -> std::path::PathBuf {
        tempfile::NamedTempFile::new()
            .expect("Failed to create temp file")
            .path()
            .to_path_buf()
    }

    pub async fn temp_server<D: DbLike>(
        path: &std::path::Path,
        config: Option<core::Config>,
    ) -> (JoinHandle<()>, CancellationToken) {
        let config = config.unwrap_or_else(|| core::Config::new(2, "test_db_".to_string()));
        let (server, cancellation_token) = core::start_server::<D>(path, config).await;
        (server, cancellation_token)
    }

    pub async fn temp_client(path: &std::path::Path) -> UnixStream {
        tokio::net::UnixStream::connect(path)
            .await
            .expect("Failed to connect")
    }
}
