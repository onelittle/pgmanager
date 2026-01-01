use std::{path::Path, process::ExitCode};

use tracing::info;

use crate::{core, stats};

pub async fn serve(path: &Path) {
    let config = core::Config::from_env();
    let (server, cancellation_token) = core::start_server(path, config).await;

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

pub async fn wrap(path: &Path, command: Vec<String>) -> ExitCode {
    let config = core::Config::from_env();
    let (server, cancellation_token) = core::start_server(path, config).await;

    // Run the command as passed and send PGMANAGER_SOCKET env var
    let command: String = command.join(" ");
    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c").arg(&command);
    cmd.env("PGMANAGER_SOCKET", path.to_str().unwrap());
    let status = cmd.status().await.unwrap();
    cancellation_token.cancel();
    server.await.unwrap();
    let exit_code: u8 = status.code().unwrap_or(1).try_into().unwrap();
    ExitCode::from(exit_code)
}

pub async fn wrap_each(
    path: &Path,
    command: Vec<String>,
    ignore_exit_code: bool,
    xarg: bool,
) -> ExitCode {
    let config = core::Config::from_env();
    let (server, cancellation_token) = core::start_server(path, config.clone()).await;
    let (program, args) = command.split_first().expect("No command provided");
    let databases = core::build_databases(config);
    let mut exit_code: u8 = 0;

    for (n, db_name) in databases.lock().await.iter().enumerate() {
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args);
        if xarg {
            cmd.arg(db_name);
        }
        cmd.env("PGDATABASE", db_name);
        cmd.env("PGM_DATABASE_SHARD", n.to_string());
        let status = cmd.status().await.unwrap();
        if !ignore_exit_code && !status.success() {
            exit_code = status
                .code()
                .unwrap_or(1)
                .try_into()
                .expect("Unable to convert exit code to u8");
            break;
        }
    }
    cancellation_token.cancel();
    server.await.unwrap();
    ExitCode::from(exit_code)
}
