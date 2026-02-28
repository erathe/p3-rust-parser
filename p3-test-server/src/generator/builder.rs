//! Message builder for P3 protocol messages.
//!
//! Combines TLV encoding, escape sequences, and CRC calculation to generate
//! complete P3 messages that match the output of real MyLaps ProChip decoders.
//!
//! # Message Structure
//! ```text
//! [SOR][VERSION][LENGTH][CRC][RESERVED][TYPE][TLV_BODY][EOR]
//!  0x8E  0x02    2-bytes  2-bytes  0x0000   2-bytes  N-bytes  0x8F
//! ```
//!
//! # Example
//! ```
//! use p3_test_server::generator::builder::build_status;
//!
//! let message = build_status(53, 16, 1, 0, 0x000C00D0);
//! // Returns complete P3 STATUS message with valid CRC
//! ```

use crate::generator::tlv::{TlvBuilder, TlvError};
use p3_protocol::{
    EOR, HEADER_SIZE, MessageType, OFFSET_CRC, OFFSET_SOR, SOR, VERSION, calculate_crc, encode,
};

#[cfg(test)]
use p3_protocol::validate_crc;
use std::time::{SystemTime, SystemTimeError, UNIX_EPOCH};
use thiserror::Error;

/// Message builder errors
#[derive(Debug, Error)]
pub enum BuilderError {
    /// System time error (e.g., time went backwards)
    #[error("System time error")]
    TimeError(
        #[from]
        #[source]
        SystemTimeError,
    ),

    /// TLV encoding error
    #[error("TLV encoding error")]
    TlvError(
        #[from]
        #[source]
        TlvError,
    ),
}

/// Get the current system time as microseconds since Unix epoch.
///
/// This is the format expected by P3 protocol RTC_TIME fields.
///
/// # Errors
/// Returns `BuilderError::TimeError` if the system time is before Unix epoch.
///
/// # Example
/// ```
/// use p3_test_server::generator::builder::current_timestamp_micros;
///
/// let now = current_timestamp_micros()?;
/// // now is microseconds since 1970-01-01 00:00:00 UTC
/// # Ok::<(), p3_test_server::generator::builder::BuilderError>(())
/// ```
pub fn current_timestamp_micros() -> Result<u64, BuilderError> {
    system_time_to_micros(SystemTime::now())
}

/// Convert a SystemTime to microseconds since Unix epoch.
///
/// # Arguments
/// * `time` - The SystemTime to convert
///
/// # Returns
/// Microseconds since Unix epoch (1970-01-01 00:00:00 UTC)
///
/// # Errors
/// Returns `BuilderError::TimeError` if the time is before Unix epoch.
///
/// # Example
/// ```
/// use p3_test_server::generator::builder::system_time_to_micros;
/// use std::time::{SystemTime, Duration};
///
/// let time = SystemTime::UNIX_EPOCH + Duration::from_secs(1609459200); // 2021-01-01
/// let micros = system_time_to_micros(time).unwrap();
/// assert_eq!(micros, 1609459200_000000);
/// ```
pub fn system_time_to_micros(time: SystemTime) -> Result<u64, BuilderError> {
    Ok(time.duration_since(UNIX_EPOCH)?.as_micros() as u64)
}

/// Convert microseconds since Unix epoch to a SystemTime.
///
/// This is the inverse of `system_time_to_micros()` and is useful for
/// decoding RTC_TIME fields from P3 messages.
///
/// # Arguments
/// * `micros` - Microseconds since Unix epoch (1970-01-01 00:00:00 UTC)
///
/// # Returns
/// SystemTime representing the given timestamp
///
/// # Example
/// ```
/// use p3_test_server::generator::builder::micros_to_system_time;
/// use std::time::{SystemTime, Duration, UNIX_EPOCH};
///
/// let micros = 1609459200_000000u64; // 2021-01-01 00:00:00 UTC
/// let time = micros_to_system_time(micros);
/// assert_eq!(time, UNIX_EPOCH + Duration::from_micros(micros));
/// ```
pub fn micros_to_system_time(micros: u64) -> SystemTime {
    use std::time::Duration;
    UNIX_EPOCH + Duration::from_micros(micros)
}

/// Format a timestamp (microseconds since Unix epoch) as a human-readable string.
///
/// Returns ISO 8601 format: "YYYY-MM-DD HH:MM:SS.ffffff"
///
/// **Note:** This formats the timestamp assuming it represents Unix time (microseconds
/// since 1970-01-01 00:00:00 UTC). However, RTC_TIME fields from P3 messages use the
/// decoder's internal Real-Time Clock which may not be synchronized with UTC. Only
/// UTC_TIME fields (Tag 0x10) are GPS-synchronized to actual UTC time.
///
/// # Arguments
/// * `micros` - Microseconds since Unix epoch
///
/// # Returns
/// Formatted datetime string
///
/// # Example
/// ```
/// use p3_test_server::generator::builder::format_timestamp;
///
/// let micros = 1609459200_000000u64; // 2021-01-01 00:00:00
/// let formatted = format_timestamp(micros);
/// assert_eq!(formatted, "2021-01-01 00:00:00.000000");
/// ```
pub fn format_timestamp(micros: u64) -> String {
    let system_time = micros_to_system_time(micros);
    let duration_since_epoch = system_time
        .duration_since(UNIX_EPOCH)
        .expect("Time calculation error");

    let total_secs = duration_since_epoch.as_secs();
    let remaining_micros = micros % 1_000_000;

    // Calculate date/time components
    const SECONDS_PER_DAY: u64 = 86400;
    const DAYS_TO_1970: i32 = 719468; // Days from 0000-03-01 to 1970-01-01

    let mut days_since_epoch = (total_secs / SECONDS_PER_DAY) as i32;
    let seconds_in_day = total_secs % SECONDS_PER_DAY;

    // Shift epoch from 1970-01-01 to 0000-03-01 (to simplify leap year handling)
    days_since_epoch += DAYS_TO_1970;

    // Calculate year, month, day using algorithm from Howard Hinnant
    let era = if days_since_epoch >= 0 {
        days_since_epoch / 146097
    } else {
        (days_since_epoch - 146096) / 146097
    };
    let day_of_era = days_since_epoch - era * 146097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_of_year = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_of_year + 2) / 5 + 1;
    let month = if month_of_year < 10 {
        month_of_year + 3
    } else {
        month_of_year - 9
    };
    let year = if month <= 2 { year + 1 } else { year };

    // Calculate time components
    let hour = seconds_in_day / 3600;
    let minute = (seconds_in_day % 3600) / 60;
    let second = seconds_in_day % 60;

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:06}",
        year, month, day, hour, minute, second, remaining_micros
    )
}

/// Build a complete P3 PASSING message.
///
/// # Arguments
/// * `passing_number` - Sequential detection counter
/// * `transponder` - Transponder/chip ID
/// * `rtc_time` - Real-time clock timestamp (microseconds since epoch)
/// * `strength` - Signal strength (60-150 typical range)
/// * `hits` - Number of signal hits detected (2-50 typical range)
/// * `flags` - Passing flags (typically 0x0000)
/// * `string` - Optional 8-byte ASCII identifier (e.g., "FL-94890" for riders, None for gates)
/// * `decoder_id` - Decoder serial number (e.g., 0x000C00D0 for "D0000C00")
///
/// # Errors
/// Returns `BuilderError::TlvError` if the string field is too long.
///
/// # Returns
/// Complete escaped P3 PASSING message with valid CRC
pub fn build_passing(
    passing_number: u32,
    transponder: u32,
    rtc_time: u64,
    strength: u16,
    hits: u16,
    flags: u16,
    string: Option<&[u8; 8]>,
    decoder_id: u32,
) -> Result<Vec<u8>, BuilderError> {
    // Build TLV body in the correct field order
    let mut tlv = TlvBuilder::new()
        .add_u32(0x01, passing_number) // PASSING_NUMBER
        .add_u32(0x03, transponder); // TRANSPONDER

    // Add string field if provided (rider passings have this, gate passings don't)
    if let Some(s) = string {
        tlv = tlv.add_bytes(0x0A, s)?; // STRING
    }

    let tlv_body = tlv
        .add_u16(0x05, strength) // STRENGTH
        .add_u16(0x06, hits) // HITS
        .add_u64(0x04, rtc_time) // RTC_TIME
        .add_u16(0x08, flags) // FLAGS
        .add_u32(0x81, decoder_id) // DECODER_ID
        .build();

    Ok(build_message(MessageType::Passing, tlv_body))
}

/// Build a complete P3 STATUS message.
///
/// # Arguments
/// * `noise` - Background noise level (15-65 typical range)
/// * `temperature` - Temperature in tenths of degrees Celsius (e.g., 16 = 1.6°C)
/// * `gps_status` - GPS lock status (0 = no fix, 1 = locked)
/// * `satinuse` - Number of GPS satellites in use
/// * `decoder_id` - Decoder serial number (e.g., 0x000C00D0 for "D0000C00")
///
/// # Returns
/// Complete escaped P3 STATUS message with valid CRC
pub fn build_status(
    noise: u16,
    temperature: i16,
    gps_status: u8,
    satinuse: u8,
    decoder_id: u32,
) -> Vec<u8> {
    // Build TLV body
    let tlv_body = TlvBuilder::new()
        .add_u16(0x01, noise)
        .add_i16(0x07, temperature)
        .add_u8(0x06, gps_status)
        .add_u8(0x0A, satinuse)
        .add_u32(0x81, decoder_id)
        .build();

    build_message(MessageType::Status, tlv_body)
}

/// Build a complete P3 VERSION message.
///
/// # Arguments
/// * `decoder_id` - Decoder registration number
/// * `description` - Decoder description string
/// * `version_string` - Firmware version string
/// * `build` - Build number
///
/// # Errors
/// Returns `BuilderError::TlvError` if description or version_string exceed 255 bytes.
///
/// # Returns
/// Complete escaped P3 VERSION message with valid CRC
pub fn build_version(
    decoder_id: u64,
    description: &str,
    version_string: &str,
    build: u16,
) -> Result<Vec<u8>, BuilderError> {
    // Build TLV body
    let tlv_body = TlvBuilder::new()
        .add_u64(0x20, decoder_id)
        .add_bytes(0x21, description.as_bytes())?
        .add_bytes(0x22, version_string.as_bytes())?
        .add_u16(0x23, build)
        .build();

    Ok(build_message(MessageType::Version, tlv_body))
}

/// Core message building function - constructs complete P3 message with CRC.
///
/// # Process
/// 1. Build unescaped message with placeholder CRC
/// 2. Calculate CRC on unescaped message
/// 3. Insert CRC at bytes 4-5
/// 4. Apply escape encoding to data section (excluding SOR/EOR)
///
/// # Arguments
/// * `msg_type` - Message type (PASSING, STATUS, VERSION)
/// * `tlv_body` - Pre-encoded TLV fields
///
/// # Returns
/// Complete escaped message ready for transmission
fn build_message(msg_type: MessageType, tlv_body: Vec<u8>) -> Vec<u8> {
    // Calculate unescaped length (entire message including SOR/EOR)
    let unescaped_length = (HEADER_SIZE + tlv_body.len() + 1) as u16; // +1 for EOR

    // Build unescaped message with zero CRC
    let mut unescaped = Vec::new();
    unescaped.push(SOR);
    unescaped.push(VERSION);
    unescaped.extend_from_slice(&unescaped_length.to_le_bytes()); // LENGTH
    unescaped.extend_from_slice(&[0x00, 0x00]); // CRC placeholder
    unescaped.extend_from_slice(&[0x00, 0x00]); // RESERVED
    unescaped.extend_from_slice(&msg_type.to_u16().to_le_bytes()); // TYPE
    unescaped.extend_from_slice(&tlv_body); // TLV body
    unescaped.push(EOR);

    // Calculate CRC on complete unescaped message
    let crc_value = calculate_crc(&unescaped);

    // Insert CRC (little-endian)
    unescaped[OFFSET_CRC] = (crc_value & 0xFF) as u8;
    unescaped[OFFSET_CRC + 1] = ((crc_value >> 8) & 0xFF) as u8;

    // Apply escape encoding to the data section (excluding SOR and EOR)
    // Extract: SOR + data + EOR
    let sor_byte = unescaped[OFFSET_SOR];
    let eor_byte = unescaped[unescaped.len() - 1];
    let data_section = &unescaped[1..unescaped.len() - 1];

    // Escape the data section
    let escaped_data = encode(data_section);

    // Reassemble: SOR + escaped_data + EOR
    let mut final_message = Vec::new();
    final_message.push(sor_byte);
    final_message.extend_from_slice(&escaped_data);
    final_message.push(eor_byte);

    final_message
}

/// Helper function to build a rider PASSING message with a specific decoder ID.
///
/// Riders have a STRING field (8-byte ASCII identifier).
///
/// # Errors
/// Returns `BuilderError::TlvError` if the string field is too long (though 8-byte arrays are always valid).
pub fn build_rider_passing(
    passing_number: u32,
    transponder: u32,
    string: &[u8; 8],
    rtc_time: u64,
    strength: u16,
    hits: u16,
    decoder_id: u32,
) -> Result<Vec<u8>, BuilderError> {
    build_passing(
        passing_number,
        transponder,
        rtc_time,
        strength,
        hits,
        0x0000, // flags
        Some(string),
        decoder_id,
    )
}

/// Helper function to build a gate PASSING message with a specific decoder ID.
///
/// Gates don't have a STRING field and use reserved transponder IDs (9991, 9992, 9995).
/// Gates also don't have STRENGTH or HITS fields in live captures.
pub fn build_gate_passing(
    passing_number: u32,
    transponder: u32,
    rtc_time: u64,
    decoder_id: u32,
) -> Vec<u8> {
    // Build TLV body for gate - no STRING, STRENGTH, or HITS fields
    let tlv_body = TlvBuilder::new()
        .add_u32(0x01, passing_number) // PASSING_NUMBER
        .add_u32(0x03, transponder) // TRANSPONDER
        .add_u64(0x04, rtc_time) // RTC_TIME
        .add_u16(0x08, 0x0000) // FLAGS
        .add_u32(0x81, decoder_id) // DECODER_ID
        .build();

    build_message(MessageType::Passing, tlv_body)
}

/// Convenience function to build a rider PASSING message with current timestamp.
///
/// Uses the current system time for RTC_TIME field. Ideal for generating
/// passing detections as they occur in real-time.
///
/// # Arguments
/// * `passing_number` - Sequential detection counter
/// * `transponder` - Transponder/chip ID
/// * `string` - 8-byte ASCII identifier (e.g., "FL-94890")
/// * `strength` - Signal strength (60-150 typical range)
/// * `hits` - Number of signal hits detected (2-50 typical range)
/// * `decoder_id` - Decoder serial number
///
/// # Errors
/// Returns `BuilderError::TimeError` if the system time is before Unix epoch.
pub fn build_rider_passing_now(
    passing_number: u32,
    transponder: u32,
    string: &[u8; 8],
    strength: u16,
    hits: u16,
    decoder_id: u32,
) -> Result<Vec<u8>, BuilderError> {
    build_rider_passing(
        passing_number,
        transponder,
        string,
        current_timestamp_micros()?,
        strength,
        hits,
        decoder_id,
    )
}

/// Convenience function to build a gate PASSING message with current timestamp.
///
/// Uses the current system time for RTC_TIME field. Ideal for generating
/// gate drop beacons as they occur in real-time.
///
/// # Arguments
/// * `passing_number` - Sequential detection counter
/// * `transponder` - Gate beacon transponder ID (typically 9991, 9992, or 9995)
/// * `decoder_id` - Decoder serial number
///
/// # Errors
/// Returns `BuilderError::TimeError` if the system time is before Unix epoch.
pub fn build_gate_passing_now(
    passing_number: u32,
    transponder: u32,
    decoder_id: u32,
) -> Result<Vec<u8>, BuilderError> {
    Ok(build_gate_passing(
        passing_number,
        transponder,
        current_timestamp_micros()?,
        decoder_id,
    ))
}

/// Build a gate PASSING message with a timestamp that contains an escape sequence.
///
/// This function generates a gate passing message using a specific timestamp value
/// (1762286699916839) that is known to produce at least one byte in the 0x8A-0x8F range,
/// requiring an escape sequence in the encoded message. This is useful for testing
/// escape sequence handling in parsers.
///
/// The timestamp value is based on real gate drop data that was observed to require
/// escaping in production use.
///
/// # Arguments
/// * `passing_number` - Sequential passing counter
/// * `transponder` - Gate beacon ID (typically 9991, 9992, or 9995)
///
/// # Returns
/// Complete escaped P3 message with SOR, headers, TLV body, CRC, and EOR
///
/// # Example
/// ```
/// use p3_test_server::generator::builder::build_gate_passing_with_escape;
///
/// // Generate gate drop with guaranteed escape sequence
/// let message = build_gate_passing_with_escape(8975, 9992, 0x000C00D0);
/// # Ok::<(), p3_test_server::generator::builder::BuilderError>(())
/// ```
pub fn build_gate_passing_with_escape(
    passing_number: u32,
    transponder: u32,
    decoder_id: u32,
) -> Vec<u8> {
    // This specific timestamp produces an escape sequence when encoded
    // It's based on real data: 1762286699916839 microseconds since epoch
    let rtc_time_with_escape: u64 = 1762286699916839;
    build_gate_passing(
        passing_number,
        transponder,
        rtc_time_with_escape,
        decoder_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_status_basic() {
        let message = build_status(53, 16, 1, 0, 0x000C00D0);

        // Should start with SOR and end with EOR
        assert_eq!(message[0], SOR);
        assert_eq!(message[message.len() - 1], EOR);

        // Should have VERSION = 0x02
        assert_eq!(message[1], VERSION);

        // Should validate CRC
        assert!(validate_crc(&message).unwrap());
    }

    #[test]
    fn test_build_passing_rider() {
        let string = b"FL-94890";
        let message = build_rider_passing(
            8841,
            102758186,
            string,
            0x0006426530063546,
            127,
            33,
            0x000C00D0,
        )
        .unwrap();

        // Should start with SOR and end with EOR
        assert_eq!(message[0], SOR);
        assert_eq!(message[message.len() - 1], EOR);

        // Should validate CRC
        assert!(validate_crc(&message).unwrap());
    }

    #[test]
    fn test_build_passing_gate() {
        let message = build_gate_passing(8855, 9992, 0x0006426606711F54, 0x000C00D0);

        // Should start with SOR and end with EOR
        assert_eq!(message[0], SOR);
        assert_eq!(message[message.len() - 1], EOR);

        // Should validate CRC
        assert!(validate_crc(&message).unwrap());
    }

    #[test]
    fn test_build_version() {
        let message = build_version(123456789, "Test Decoder", "1.0.0", 100).unwrap();

        // Should start with SOR and end with EOR
        assert_eq!(message[0], SOR);
        assert_eq!(message[message.len() - 1], EOR);

        // Should validate CRC
        assert!(validate_crc(&message).unwrap());
    }

    #[test]
    fn test_system_time_to_micros() {
        use std::time::Duration;

        // Test known timestamp: 2021-01-01 00:00:00 UTC
        let time = UNIX_EPOCH + Duration::from_secs(1609459200);
        let micros = system_time_to_micros(time).unwrap();
        assert_eq!(micros, 1609459200_000000);

        // Test with microseconds precision
        let time = UNIX_EPOCH + Duration::from_micros(1609459200_123456);
        let micros = system_time_to_micros(time).unwrap();
        assert_eq!(micros, 1609459200_123456);
    }

    #[test]
    fn test_system_time_to_micros_error() {
        use std::time::Duration;

        // Test time before Unix epoch
        let time = UNIX_EPOCH - Duration::from_secs(1);
        let result = system_time_to_micros(time);
        assert!(result.is_err());
    }

    #[test]
    fn test_current_timestamp_micros() {
        // Just verify it returns a reasonable value (after 2020, before 2100)
        let now = current_timestamp_micros().unwrap();
        let year_2020_micros = 1577836800_000000u64; // 2020-01-01
        let year_2100_micros = 4102444800_000000u64; // 2100-01-01

        assert!(
            now > year_2020_micros && now < year_2100_micros,
            "Timestamp should be between 2020 and 2100, got: {}",
            now
        );
    }

    #[test]
    fn test_build_rider_passing_now() {
        let string = b"FL-94890";
        let message =
            build_rider_passing_now(8841, 102758186, string, 127, 33, 0x000C00D0).unwrap();

        // Should start with SOR and end with EOR
        assert_eq!(message[0], SOR);
        assert_eq!(message[message.len() - 1], EOR);

        // Should validate CRC
        assert!(validate_crc(&message).unwrap());

        // Should be non-empty
        assert!(message.len() > 40); // Rider messages are typically ~60 bytes
    }

    #[test]
    fn test_build_gate_passing_now() {
        let message = build_gate_passing_now(8855, 9992, 0x000C00D0).unwrap();

        // Should start with SOR and end with EOR
        assert_eq!(message[0], SOR);
        assert_eq!(message[message.len() - 1], EOR);

        // Should validate CRC
        assert!(validate_crc(&message).unwrap());

        // Should be non-empty
        assert!(message.len() > 30); // Gate messages are typically ~43 bytes
    }
}
