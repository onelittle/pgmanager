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

pub async fn get_database() -> Result<DatabaseGuard, Box<dyn std::error::Error>> {
    let path = util::env_var_with_fallback("PGM_SOCKET", "PGMANAGER_SOCKET")
        .unwrap_or_else(|| DEFAULT_SOCKET_PATH.to_string());
    let stream = tokio::net::UnixStream::connect(path)
        .await
        .expect("Failed to connect to test manager socket");
    get_database_from_stream(stream).await
}

async fn get_database_from_stream(
    mut stream: UnixStream,
) -> Result<DatabaseGuard, Box<dyn std::error::Error>> {
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
