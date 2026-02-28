use clap::Parser;
use p3_test_server::generator::builder::current_timestamp_micros;
use p3_test_server::simulator::DecoderSimulator;
use p3_test_server::transport::TcpTransport;
use rand::rngs::SmallRng;
use rand::{Rng, SeedableRng};
use tracing::info;

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

    /// Number of riders for the full-race scenario (3-8)
    #[arg(long, default_value = "6")]
    riders: usize,
}

/// Simulated decoder IDs for different timing loops
const DECODER_GATE: u32 = 0x000C00D0; // D0000C00 — gate/start area
const DECODER_START_HILL: u32 = 0x000C00D1; // D1000C00 — start hill
const DECODER_CORNER1: u32 = 0x000C00D2; // D2000C00 — corner 1
const DECODER_FINISH: u32 = 0x000C00D3; // D3000C00 — finish line

/// Simulated rider data
struct SimRider {
    transponder_id: u32,
    string: [u8; 8],
    /// Base speed factor (1.0 = average, lower = faster)
    speed_factor: f64,
}

fn make_riders(count: usize) -> Vec<SimRider> {
    let rider_data: Vec<(u32, &[u8; 8], f64)> = vec![
        (1001, b"FL-01001", 0.95), // fast rider
        (1002, b"FL-01002", 1.00), // average
        (1003, b"FL-01003", 1.03), // slightly slower
        (1004, b"FL-01004", 0.98), // above average
        (1005, b"FL-01005", 1.05), // slower
        (1006, b"FL-01006", 1.01), // average
        (1007, b"FL-01007", 0.97), // above average
        (1008, b"FL-01008", 1.08), // slowest
    ];

    rider_data
        .into_iter()
        .take(count.min(8).max(3))
        .map(|(tid, s, sf)| SimRider {
            transponder_id: tid,
            string: *s,
            speed_factor: sf,
        })
        .collect()
}

/// Compute per-rider offsets (in ms) for a timing loop crossing.
/// `total_spread_ms` is the max spread across the whole pack.
/// Returns (rider_index, offset_ms) sorted by offset so we send in time order.
fn compute_offsets(
    riders: &[SimRider],
    total_spread_ms: f64,
    rng: &mut SmallRng,
) -> Vec<(usize, u64)> {
    let mut offsets: Vec<(usize, u64)> = riders
        .iter()
        .enumerate()
        .map(|(i, r)| {
            // Base position from speed (faster rider = lower offset)
            let base = (r.speed_factor - 0.95) / 0.13 * total_spread_ms;
            // Add some random jitter (±15% of spread)
            let jitter: f64 = rng.random_range(-0.15..0.15) * total_spread_ms;
            let offset = (base + jitter).max(0.0) as u64;
            (i, offset)
        })
        .collect();
    // Sort by offset so we send them in chronological order
    offsets.sort_by_key(|&(_, ms)| ms);
    offsets
}

/// Send rider passings for a single timing loop section.
async fn send_section(
    sim: &DecoderSimulator,
    riders: &[SimRider],
    offsets: &[(usize, u64)],
    decoder_id: u32,
    rng: &mut SmallRng,
) {
    let mut prev_offset = 0u64;
    for &(rider_idx, offset_ms) in offsets {
        // Sleep the delta from the previous rider
        let delta = offset_ms.saturating_sub(prev_offset);
        if delta > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(delta)).await;
        }
        prev_offset = offset_ms;

        let rider = &riders[rider_idx];
        let strength: u16 = rng.random_range(95..145);
        let hits: u16 = rng.random_range(18..48);
        if let Err(e) = sim
            .send_rider_passing(
                rider.transponder_id,
                &rider.string,
                strength,
                hits,
                Some(decoder_id),
            )
            .await
        {
            tracing::error!(
                "Failed to send passing for transponder {}: {}",
                rider.transponder_id,
                e
            );
        }
    }
}

/// Run the full-race scenario with multiple loops
async fn run_full_race(sim: DecoderSimulator, rider_count: usize) {
    let riders = make_riders(rider_count);
    let mut rng = SmallRng::from_os_rng();

    info!(
        "Full-race scenario: {} riders, 4 loops (gate, start hill, corner 1, finish)",
        riders.len()
    );
    info!("Waiting 5s for clients to connect...");
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    loop {
        info!("=== RACE START (gate drop in 3s) ===");
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

        // ---- Gate drop ----
        let gate_time_us = current_timestamp_micros().unwrap();
        info!("GATE DROP!");
        if let Err(e) = sim.send_gate_passing(9992, Some(DECODER_GATE)).await {
            tracing::error!("Failed to send gate drop: {}", e);
            return;
        }

        // ---- Start hill (~0.9s after gate, riders bunched within ~200ms) ----
        // In BMX all 8 riders come off the gate together — very tight pack
        let start_hill_delay_ms: u64 = 900;
        tokio::time::sleep(tokio::time::Duration::from_millis(start_hill_delay_ms)).await;
        info!("Riders crossing start hill...");
        let offsets = compute_offsets(&riders, 200.0, &mut rng);
        send_section(&sim, &riders, &offsets, DECODER_START_HILL, &mut rng).await;

        // ---- Corner 1 (~3.5s after start hill, riders spread within ~600ms) ----
        // Pack is starting to separate through the first straight and turn
        let corner1_delay_ms: u64 = 3500;
        tokio::time::sleep(tokio::time::Duration::from_millis(corner1_delay_ms)).await;
        info!("Riders approaching corner 1...");
        let offsets = compute_offsets(&riders, 600.0, &mut rng);
        send_section(&sim, &riders, &offsets, DECODER_CORNER1, &mut rng).await;

        // ---- Finish line (~4s after corner 1, riders spread within ~1.5s) ----
        // Biggest gaps at the end — leaders pull away, back of pack strung out
        let finish_delay_ms: u64 = 4000;
        tokio::time::sleep(tokio::time::Duration::from_millis(finish_delay_ms)).await;
        info!("Riders approaching finish...");
        let offsets = compute_offsets(&riders, 1500.0, &mut rng);
        send_section(&sim, &riders, &offsets, DECODER_FINISH, &mut rng).await;

        let elapsed_ms = (current_timestamp_micros().unwrap() - gate_time_us) / 1000;
        info!(
            "=== RACE FINISHED ({:.1}s elapsed) ===",
            elapsed_ms as f64 / 1000.0
        );

        // Wait before next race
        info!("Next race in 15s...");
        tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;
    }
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
    info!("Press Ctrl+C to stop");

    // Run scenario based on CLI argument
    match args.scenario.as_str() {
        "idle" => {
            info!("Running idle scenario (STATUS messages only)");
        }
        "bmx-race" => {
            info!("Running BMX race scenario (simple 2-rider demo)");
            tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

                info!("Sending gate drop");
                if let Err(e) = simulator.send_gate_passing(9992, None).await {
                    tracing::error!("Failed to send gate passing: {}", e);
                }

                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                if let Err(e) = simulator.send_gate_passing_with_escape(9992).await {
                    tracing::error!("Failed to send gate passing with escape: {}", e);
                }

                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                if let Err(e) = simulator
                    .send_rider_passing(102758186, b"FL-94890", 127, 33, None)
                    .await
                {
                    tracing::error!("Failed to send rider passing: {}", e);
                }

                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                if let Err(e) = simulator
                    .send_rider_passing(123456789, b"FL-12345", 120, 45, None)
                    .await
                {
                    tracing::error!("Failed to send rider passing: {}", e);
                }
            });
        }
        "full-race" => {
            let rider_count = args.riders;
            info!(
                "Running full-race scenario ({} riders, looping)",
                rider_count
            );
            tokio::spawn(run_full_race(simulator, rider_count));
        }
        _ => {
            tracing::warn!("Unknown scenario '{}', running idle", args.scenario);
        }
    }

    // Run the transport server
    transport.run().await?;

    Ok(())
}
