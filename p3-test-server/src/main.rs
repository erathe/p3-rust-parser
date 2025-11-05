use clap::Parser;
use p3_test_server::simulator::DecoderSimulator;
use p3_test_server::transport::TcpTransport;
use tracing::info;
use tracing_subscriber;

#[derive(Parser, Debug)]
#[command(name = "P3 Test Server")]
#[command(about = "MyLaps ProChip P3 Protocol Test Server", long_about = None)]
struct Args {
    #[arg(short, long, default_value = "5403")]
    port: u16,

    #[arg(short, long, default_value = "idle")]
    scenario: String,

    #[arg(long, default_value = "4")]
    max_clients: usize,

    #[arg(long)]
    chunk_size: Option<usize>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_level(true)
        .init();

    let args = Args::parse();

    info!("P3 Test Server");
    info!("Port: {}", args.port);
    info!("Scenario: {}", args.scenario);
    info!("Max clients: {}", args.max_clients);

    let (transport, handle) =
        TcpTransport::new(args.port, args.max_clients, args.chunk_size).await?;

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

                // Gate drop (normal)
                info!("Sending gate drop (normal)");
                if let Err(e) = simulator.send_gate_passing(9992).await {
                    tracing::error!("Failed to send gate passing: {}", e);
                }

                // Gate drop (with escape sequence) - this would previously fail with "invalid frame"
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                info!("Sending gate drop WITH ESCAPE SEQUENCE (tests the parser fix)");
                if let Err(e) = simulator.send_gate_passing_with_escape(9992).await {
                    tracing::error!("Failed to send gate passing with escape: {}", e);
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
