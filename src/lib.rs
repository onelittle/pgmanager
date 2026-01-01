pub mod commands;
mod core;
mod stats;
mod util;

use tokio::{io::AsyncReadExt, net::UnixStream};

pub const DEFAULT_SOCKET_PATH: &str = "tmp/test_manager.sock";

pub struct DatabaseGuard {
    pub name: String,
    _stream: UnixStream,
}

pub async fn get_database() -> DatabaseGuard {
    let path = util::env_var_with_fallback("PGM_SOCKET", "PGMANAGER_SOCKET")
        .unwrap_or_else(|| DEFAULT_SOCKET_PATH.to_string());
    let stream = tokio::net::UnixStream::connect(path)
        .await
        .expect("Failed to connect to test manager socket");
    get_database_from_stream(stream).await
}

async fn get_database_from_stream(mut stream: UnixStream) -> DatabaseGuard {
    let mut buffer = [0; 1024];
    let read = stream
        .read(&mut buffer)
        .await
        .expect("Failed to read from test manager socket");
    if read == 0 {
        panic!("Test manager socket closed unexpectedly");
    }
    let response = String::from_utf8_lossy(&buffer);
    let (prefix, message) = response.split_once(':').unwrap_or(("", ""));
    match (prefix, message) {
        ("OK", db_name) => {
            let db_name = db_name.replace('\0', "");

            eprintln!("Using test database: {}", db_name);
            DatabaseGuard {
                name: db_name,
                _stream: stream,
            }
        }
        ("EMPTY", message) => {
            panic!("No databases available: {message}");
        }
        (_, _) => {
            panic!("Unexpected response from test manager: {response}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_database() {
        let path = test_helpers::temp_path();
        let (server, cancellation_token) = test_helpers::temp_server(&path).await;

        let stream = test_helpers::temp_client(&path).await;
        let db_guard_a = get_database_from_stream(stream).await;
        let stream = test_helpers::temp_client(&path).await;
        let db_guard_b = get_database_from_stream(stream).await;

        assert!(db_guard_a.name.starts_with("test_db_"));
        assert!(db_guard_b.name.starts_with("test_db_"));
        assert_ne!(db_guard_a.name, db_guard_b.name);
        cancellation_token.cancel();
        server.await.expect("Server task failed");
    }

    mod test_helpers {
        use tokio::{net::UnixStream, task::JoinHandle};
        use tokio_util::sync::CancellationToken;

        use crate::core;

        pub fn temp_path() -> std::path::PathBuf {
            tempfile::NamedTempFile::new()
                .expect("Failed to create temp file")
                .path()
                .to_path_buf()
        }

        pub async fn temp_server(path: &std::path::Path) -> (JoinHandle<()>, CancellationToken) {
            let config = core::Config::new(2, "test_db_".to_string());
            let (server, cancellation_token) = core::start_server(path, config).await;
            (server, cancellation_token)
        }

        pub async fn temp_client(path: &std::path::Path) -> UnixStream {
            tokio::net::UnixStream::connect(path)
                .await
                .expect("Failed to connect")
        }
    }
}
