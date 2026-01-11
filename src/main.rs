use std::path::PathBuf;

use clap::{Parser, Subcommand};

use pgmanager::{DatabaseConfig, commands};
use pgtemp::PgTempDB;

#[derive(Parser)]
struct Cli {
    /// Path to the Unix domain socket
    #[clap(short, long, default_value = pgmanager::DEFAULT_SOCKET_PATH)]
    socket: String,
    /// Enable verbose logging
    #[clap(short, long, default_value_t = false)]
    verbose: bool,
    /// Use pgtemp for temporary databases
    #[clap(long, default_value_t = false)]
    pgtemp: bool,
    // Subcommand to wrap another command
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Serve the pgmanager socket
    #[command()]
    Serve,
    /// Wrap a command and pass PGMANAGER_SOCKET
    #[command()]
    Wrap {
        #[arg(last = true)]
        command: Vec<String>,
    },
    /// Wrap a command n times passing PGM_SHARD and PGM_DATABASE_SHARD
    #[command()]
    WrapEach {
        #[arg(last = true)]
        command: Vec<String>,
        #[arg(short, long, default_value_t = false)]
        ignore_exit_code: bool,
        /// Pass the database name as an argument
        #[arg(short, long, default_value_t = false)]
        xarg: bool,
    },
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let args = Cli::parse();

    tracing_subscriber::fmt()
        .with_max_level(if args.verbose {
            tracing::Level::DEBUG
        } else {
            tracing::Level::INFO
        })
        .with_writer(
            // Write to stderr
            std::io::stderr,
        )
        .init();

    let path = if args.socket.starts_with("/") {
        PathBuf::from(args.socket)
    } else {
        std::env::current_dir().unwrap().join(args.socket)
    };

    match (args.command, args.pgtemp) {
        (Commands::Serve, true) => {
            commands::serve::<pgtemp::PgTempDB>(&path).await;
            std::process::ExitCode::SUCCESS
        }
        (Commands::Wrap { command }, true) => commands::wrap::<PgTempDB>(&path, command).await,
        (Commands::Serve, false) => {
            commands::serve::<DatabaseConfig>(&path).await;
            std::process::ExitCode::SUCCESS
        }
        (Commands::Wrap { command }, false) => {
            commands::wrap::<DatabaseConfig>(&path, command).await
        }
        (
            Commands::WrapEach {
                command,
                ignore_exit_code,
                xarg,
            },
            false,
        ) => commands::wrap_each(&path, command, ignore_exit_code, xarg).await,
        (_, true) => {
            eprintln!("The --pgtemp flag is not supported with this command.");
            std::process::ExitCode::from(1)
        }
    }
}
