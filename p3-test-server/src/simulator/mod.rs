//! Decoder simulator for generating P3 protocol messages

mod state;

pub use state::DecoderState;

use crate::generator::builder::{
    build_gate_passing, build_gate_passing_with_escape, build_rider_passing, build_status,
    current_timestamp_micros,
};
use crate::transport::{SendError, TransportHandle};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{Duration, interval};
use tracing::{debug, error, info};

/// Decoder simulator that generates and sends P3 protocol messages
#[derive(Clone)]
pub struct DecoderSimulator {
    state: Arc<Mutex<DecoderState>>,
    handle: TransportHandle,
}

impl DecoderSimulator {
    pub fn new(handle: TransportHandle) -> Self {
        Self {
            state: Arc::new(Mutex::new(DecoderState::default())),
            handle,
        }
    }

    pub fn with_state(handle: TransportHandle, state: DecoderState) -> Self {
        Self {
            state: Arc::new(Mutex::new(state)),
            handle,
        }
    }

    pub async fn start_status_loop(self) {
        let mut timer = interval(Duration::from_secs(5));

        info!("Starting STATUS message loop (5 second interval)");

        loop {
            timer.tick().await;

            if let Err(e) = self.send_status().await {
                error!("Failed to send STATUS message: {}", e);
            }
        }
    }

    pub async fn send_status(&self) -> Result<(), SendError> {
        let state = self.state.lock().await;

        let message = build_status(
            state.noise_level,
            state.temperature_celsius_x10,
            state.gps_has_fix as u8,
            state.gps_satellites,
            state.decoder_id,
        );

        debug!(
            "Sending STATUS: noise={}, temp={}, gps={}, sats={}",
            state.noise_level,
            state.temperature_celsius_x10,
            state.gps_has_fix as u8,
            state.gps_satellites
        );

        self.handle.send(message).await
    }

    /// Send a rider PASSING message on a specific decoder
    ///
    /// # Arguments
    /// * `transponder` - Transponder ID
    /// * `string` - 8-byte ASCII identifier (e.g., b"FL-94890")
    /// * `strength` - Signal strength (60-150 typical)
    /// * `hits` - Number of signal hits (2-50 typical)
    /// * `decoder_id` - Optional decoder ID override (uses state default if None)
    pub async fn send_rider_passing(
        &self,
        transponder: u32,
        string: &[u8; 8],
        strength: u16,
        hits: u16,
        decoder_id: Option<u32>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut state = self.state.lock().await;

        let passing_number = state.next_passing_number();
        let rtc_time = current_timestamp_micros()?;
        let did = decoder_id.unwrap_or(state.decoder_id);

        let message = build_rider_passing(
            passing_number,
            transponder,
            string,
            rtc_time,
            strength,
            hits,
            did,
        )?;

        debug!(
            "Sending PASSING (rider): passing_number={}, transponder={}, string={}, decoder={:#010X}",
            passing_number,
            transponder,
            String::from_utf8_lossy(string),
            did
        );

        self.handle.send(message).await?;
        Ok(())
    }

    /// Send a gate beacon PASSING message
    ///
    /// # Arguments
    /// * `transponder` - Gate beacon ID (typically 9991, 9992, or 9995)
    /// * `decoder_id` - Optional decoder ID override (uses state default if None)
    pub async fn send_gate_passing(
        &self,
        transponder: u32,
        decoder_id: Option<u32>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut state = self.state.lock().await;

        let passing_number = state.next_passing_number();
        let rtc_time = current_timestamp_micros()?;
        let did = decoder_id.unwrap_or(state.decoder_id);

        let message = build_gate_passing(passing_number, transponder, rtc_time, did);

        debug!(
            "Sending PASSING (gate): passing_number={}, transponder={}, decoder={:#010X}",
            passing_number, transponder, did
        );

        self.handle.send(message).await?;
        Ok(())
    }

    /// Send a gate beacon PASSING message with guaranteed escape sequence
    ///
    /// # Arguments
    /// * `transponder` - Gate beacon ID (typically 9991, 9992, or 9995)
    pub async fn send_gate_passing_with_escape(
        &self,
        transponder: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut state = self.state.lock().await;

        let passing_number = state.next_passing_number();
        let did = state.decoder_id;

        let message = build_gate_passing_with_escape(passing_number, transponder, did);

        debug!(
            "Sending PASSING (gate with escape): passing_number={}, transponder={}",
            passing_number, transponder
        );

        self.handle.send(message).await?;
        Ok(())
    }

    /// Get a reference to the decoder state (for inspection/modification)
    pub fn state(&self) -> Arc<Mutex<DecoderState>> {
        Arc::clone(&self.state)
    }
}
