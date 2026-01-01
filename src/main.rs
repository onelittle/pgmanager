use std::path::PathBuf;

use clap::{Parser, Subcommand, command};

use pgmanager::{serve, wrap};

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
    /// Serve the pgmanager socket
    #[command()]
    Serve,
    /// Wrap a command and pass PGMANAGER_SOCKET
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
            serve(&path).await;
        }
        Commands::Wrap { command } => {
            wrap(&path, command).await;
        }
    }
}
