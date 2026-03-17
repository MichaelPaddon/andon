use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use tracing::info;
use tracing_subscriber::EnvFilter;

use andon::config::Config;

/// Signal-routing daemon.
///
/// Reads a TOML config file, wires sources through nodes
/// to sinks, and runs until interrupted.
#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to the TOML configuration file.
    config: PathBuf,
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| {
                    EnvFilter::new("debug")
                }),
        )
        .init();

    let args = Args::parse();

    let text =
        match std::fs::read_to_string(&args.config) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "error: cannot read {}: {}",
                    args.config.display(),
                    e
                );
                return ExitCode::FAILURE;
            }
        };

    let config = match Config::from_str(&text) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {}", e);
            return ExitCode::FAILURE;
        }
    };

    let bus = match config.build() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: {}", e);
            return ExitCode::FAILURE;
        }
    };

    let (handles, shutdown) = match bus.run().await {
        Ok(pair) => pair,
        Err(missing) => {
            for name in &missing {
                eprintln!(
                    "error: unknown sink {:?}",
                    name
                );
            }
            return ExitCode::FAILURE;
        }
    };

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("received shutdown signal");
        }
        _ = async move {
            for h in handles {
                let _ = h.await;
            }
        } => {}
    }

    shutdown.shutdown().await;

    ExitCode::SUCCESS
}
