use clap::{Parser, Subcommand};
use p3_test_server::generator::builder::build_version;
use p3_test_server::simulator::DecoderSimulator;
use p3_test_server::transport::TcpTransport;
use p3_test_server::tui::{self, TuiOptions};
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "P3 Test Server")]
#[command(about = "MyLaps ProChip P3 Protocol Test Server", long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,

    #[arg(short, long, default_value = "5403")]
    port: u16,

    #[arg(short, long, default_value = "idle")]
    scenario: String,

    #[arg(long, default_value = "4")]
    max_clients: usize,

    #[arg(long)]
    chunk_size: Option<usize>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Interactive race-control TUI (roster and settings persist in SQLite)
    Tui {
        #[arg(short, long, default_value = "5403")]
        port: u16,

        #[arg(long, default_value = "4")]
        max_clients: usize,

        #[arg(long)]
        chunk_size: Option<usize>,

        /// SQLite database holding roster and settings
        #[arg(long, default_value = "test-server.db")]
        db: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    if let Some(Command::Tui {
        port,
        max_clients,
        chunk_size,
        db,
    }) = args.command
    {
        return tui::run(TuiOptions {
            port,
            max_clients,
            chunk_size,
            db_path: db,
        })
        .await;
    }

    run_headless(args).await
}

/// Scripted scenario mode (unchanged behavior, used by CI and scripts)
async fn run_headless(args: Args) -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .init();

    info!("P3 Test Server");
    info!("Port: {}", args.port);
    info!("Scenario: {}", args.scenario);
    info!("Max clients: {}", args.max_clients);

    // Real decoders send a VERSION message when a client connects
    let greeting = build_version(
        0x000C00D0,
        "MyLaps Test Decoder (sim)",
        env!("CARGO_PKG_VERSION"),
        1,
    )?;

    let (transport, handle, _registry) =
        TcpTransport::new(args.port, args.max_clients, args.chunk_size, Some(greeting.into()))
            .await?;

    let simulator = DecoderSimulator::new(handle);

    let sim_clone = simulator.clone();
    tokio::spawn(async move {
        sim_clone.start_status_loop().await;
    });

    info!("Starting server...");
    info!("Scenario: {}", args.scenario);
    info!("Press Ctrl+C to stop");

    // Run scenario based on CLI argument
    match args.scenario.as_str() {
        "idle" => {
            info!("Running idle scenario (STATUS messages only)");
        }
        "bmx-race" => {
            info!("Running BMX race scenario");
            // Spawn a task to run a simple BMX race scenario
            tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

                // Gate drop
                info!("Sending gate drop");
                if let Err(e) = simulator.send_gate_passing(9992).await {
                    tracing::error!("Failed to send gate passing: {}", e);
                }

                // Rider 1
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                if let Err(e) = simulator
                    .send_rider_passing(102758186, b"FL-94890", 127, 33)
                    .await
                {
                    tracing::error!("Failed to send rider passing: {}", e);
                }

                // Rider 2
                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                if let Err(e) = simulator
                    .send_rider_passing(123456789, b"FL-12345", 120, 45)
                    .await
                {
                    tracing::error!("Failed to send rider passing: {}", e);
                }
            });
        }
        _ => {
            tracing::warn!("Unknown scenario '{}', running idle", args.scenario);
        }
    }

    // Run the transport server
    transport.run().await?;

    Ok(())
}
