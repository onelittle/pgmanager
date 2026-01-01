use std::path::PathBuf;

use clap::{Parser, Subcommand, command};

use pgmanager::commands;

#[derive(Parser)]
struct Cli {
    /// Path to the Unix domain socket
    #[clap(short, long, default_value = pgmanager::DEFAULT_SOCKET_PATH)]
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

    match args.command {
        Commands::Serve => {
            commands::serve(&path).await;
            std::process::ExitCode::SUCCESS
        }
        Commands::Wrap { command } => commands::wrap(&path, command).await,
        Commands::WrapEach {
            command,
            ignore_exit_code,
            xarg,
        } => commands::wrap_each(&path, command, ignore_exit_code, xarg).await,
    }
}
