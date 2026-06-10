//! Fault injection: deliberately broken wire data for hardening clients.
//!
//! A robust race client must survive corruption without losing subsequent
//! messages. These functions produce the three failure shapes a TCP client
//! can encounter: a complete frame with a bad CRC, raw line noise, and a
//! frame that never finishes.

use p3_protocol::{OFFSET_CRC, encode, unescape_data};
use rand::Rng;

/// Return a copy of a valid escaped message with its CRC field corrupted.
///
/// The frame stays structurally valid (SOR/EOR intact, escapes correct), so
/// a client should reach CRC validation and reject it there — then keep
/// parsing the stream.
///
/// # Panics
/// Panics if `message` is not a valid escaped P3 frame; callers pass
/// freshly built messages.
pub fn corrupt_crc(message: &[u8]) -> Vec<u8> {
    let mut unescaped = unescape_data(message).expect("input must be a valid escaped message");
    assert!(unescaped.len() > OFFSET_CRC + 1, "frame too short");

    unescaped[OFFSET_CRC] ^= 0xFF;

    // Re-escape the data section between SOR and EOR (the corrupted CRC byte
    // may now be a control byte; encode() handles that)
    let last = unescaped.len() - 1;
    let mut out = Vec::with_capacity(message.len());
    out.push(unescaped[0]);
    out.extend(encode(&unescaped[1..last]));
    out.push(unescaped[last]);
    out
}

/// Random line noise containing no control bytes (no SOR/EOR/ESCAPE).
///
/// A client should discard this without producing any message.
pub fn garbage_bytes(len: usize) -> Vec<u8> {
    let mut rng = rand::thread_rng();
    (0..len).map(|_| rng.gen_range(0x00..0x8A)).collect()
}

/// A valid frame cut off before its EOR.
///
/// A client should discard the fragment when the next frame's SOR arrives.
pub fn truncate_frame(message: &[u8]) -> Vec<u8> {
    message[..message.len() * 2 / 3].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::builder::{build_gate_passing, build_status};
    use p3_protocol::{EOR, SOR, validate_crc};

    #[test]
    fn corrupt_crc_keeps_structure_but_fails_validation() {
        // Vary content so corrupted CRC bytes land in different ranges,
        // including the control-byte range that needs re-escaping
        for noise in 0..200u16 {
            let message = build_status(noise, 16, 1, 0, 0x000C00D0);
            let corrupted = corrupt_crc(&message);

            assert_eq!(corrupted[0], SOR);
            assert_eq!(*corrupted.last().unwrap(), EOR);
            assert_ne!(corrupted, message);
            assert!(
                !validate_crc(&corrupted).expect("frame must stay structurally valid"),
                "corrupted frame must fail CRC validation (noise={})",
                noise
            );
        }
    }

    #[test]
    fn corrupt_crc_works_on_passing_messages() {
        let message = build_gate_passing(8855, 9992, 0x0006426606711F54);
        let corrupted = corrupt_crc(&message);
        assert!(!validate_crc(&corrupted).unwrap());
    }

    #[test]
    fn garbage_contains_no_control_bytes() {
        let garbage = garbage_bytes(4096);
        assert_eq!(garbage.len(), 4096);
        assert!(
            garbage.iter().all(|&b| !(0x8A..=0x8F).contains(&b)),
            "garbage must not contain frame markers"
        );
    }

    #[test]
    fn truncated_frame_has_no_eor() {
        let message = build_status(53, 16, 1, 0, 0x000C00D0);
        let truncated = truncate_frame(&message);
        assert_eq!(truncated[0], SOR);
        assert!(truncated.len() < message.len());
        assert!(
            !truncated.contains(&EOR),
            "truncated frame must not contain EOR"
        );
    }
}
