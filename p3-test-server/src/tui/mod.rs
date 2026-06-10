//! Interactive race-control TUI.
//!
//! Drives the decoder simulator like a race official: drop the gate, send
//! riders across the line, inject faults, and edit decoder state — all
//! persisted to SQLite so the setup survives restarts.

mod app;
mod log;
mod ui;

use crate::config::Db;
use crate::generator::builder::build_version;
use crate::simulator::{DecoderSimulator, DecoderState};
use crate::transport::TcpTransport;
use app::App;
use crossterm::event::{Event, KeyEventKind};
use std::time::Duration;
use tokio::sync::mpsc;

pub struct TuiOptions {
    pub port: u16,
    pub max_clients: usize,
    pub chunk_size: Option<usize>,
    pub db_path: String,
}

pub async fn run(opts: TuiOptions) -> anyhow::Result<()> {
    // Config from SQLite (created and seeded on first run)
    let db = Db::open(&opts.db_path)?;
    let settings = db.load_settings()?;
    let riders = db.load_riders()?;

    // Route tracing into the TUI log pane — nothing may write to stdout
    let (log_tx, mut log_rx) = mpsc::unbounded_channel();
    tracing_subscriber::fmt()
        .with_writer(log::ChannelWriter::new(log_tx))
        .with_ansi(false)
        .without_time()
        .with_target(false)
        .init();

    // Transport, greeting new clients with VERSION like a real decoder
    let greeting = build_version(
        settings.decoder_id as u64,
        "MyLaps Test Decoder (sim)",
        env!("CARGO_PKG_VERSION"),
        1,
    )?;
    let (transport, handle, registry) = TcpTransport::new(
        opts.port,
        opts.max_clients,
        opts.chunk_size,
        Some(greeting.into()),
    )
    .await?;
    tokio::spawn(transport.run());

    // Simulator seeded from persisted settings
    let state = DecoderState {
        decoder_id: settings.decoder_id,
        passing_number: 0,
        noise_level: settings.noise,
        temperature_celsius_x10: settings.temperature_x10,
        gps_has_fix: settings.gps_fix,
        gps_satellites: settings.satellites,
        status_interval_s: settings.status_interval_s,
        status_paused: false,
    };
    let sim = DecoderSimulator::with_state(handle.clone(), state);
    tokio::spawn(sim.clone().start_status_loop());

    let mut app = App::new(
        sim,
        handle,
        registry,
        db,
        settings,
        riders,
        opts.port,
        opts.chunk_size,
    );

    // Crossterm events read on a blocking thread, forwarded into the loop
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        while let Ok(event) = crossterm::event::read() {
            if event_tx.send(event).is_err() {
                break;
            }
        }
    });

    let mut terminal = ratatui::init();
    let result = run_loop(&mut terminal, &mut app, &mut event_rx, &mut log_rx).await;
    ratatui::restore();
    result
}

async fn run_loop(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    event_rx: &mut mpsc::UnboundedReceiver<Event>,
    log_rx: &mut mpsc::UnboundedReceiver<String>,
) -> anyhow::Result<()> {
    let mut tick = tokio::time::interval(Duration::from_millis(100));

    loop {
        app.tick();
        terminal.draw(|frame| ui::render(frame, app))?;

        tokio::select! {
            Some(event) = event_rx.recv() => {
                if let Event::Key(key) = event
                    && key.kind == KeyEventKind::Press
                {
                    app.handle_key(key).await;
                }
            }
            Some(line) = log_rx.recv() => {
                app.push_log(line);
                // Drain whatever else is queued so bursts render together
                while let Ok(line) = log_rx.try_recv() {
                    app.push_log(line);
                }
            }
            _ = tick.tick() => {}
        }

        if app.should_quit {
            return Ok(());
        }
    }
}
