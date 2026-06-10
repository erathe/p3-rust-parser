//! Routes `tracing` output into the TUI event-log pane.
//!
//! Ratatui owns the terminal, so log lines must not hit stdout. A
//! `ChannelWriter` is installed as the tracing writer; the TUI event loop
//! drains the channel into the visible log buffer.

use std::io;
use tokio::sync::mpsc;
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone)]
pub struct ChannelWriter {
    tx: mpsc::UnboundedSender<String>,
}

impl ChannelWriter {
    pub fn new(tx: mpsc::UnboundedSender<String>) -> Self {
        Self { tx }
    }
}

impl io::Write for ChannelWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        for line in text.lines() {
            if !line.trim().is_empty() {
                // Receiver gone means the TUI is shutting down; drop silently
                let _ = self.tx.send(line.to_string());
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for ChannelWriter {
    type Writer = ChannelWriter;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}
