//! Message type definitions for P3 protocol

use crate::error::{ParseError, ParseResult};
use crate::tlv::{TlvDecoder, TlvField};
use p3_protocol::fields::{passing, status, version};
use serde::{Deserialize, Serialize};

// Helper functions for common TLV field operations

/// Format a 4-byte decoder ID as a hex string in wire order (little-endian)
fn format_decoder_id_u32(bytes: &[u8]) -> Option<String> {
    if bytes.len() == 4 {
        Some(format!(
            "{:02X}{:02X}{:02X}{:02X}",
            bytes[0], bytes[1], bytes[2], bytes[3]
        ))
    } else {
        None
    }
}

/// Format an 8-byte decoder ID as a hex string in wire order (little-endian)
fn format_decoder_id_u64(bytes: &[u8]) -> Option<String> {
    if bytes.len() == 8 {
        Some(format!(
            "{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]
        ))
    } else {
        None
    }
}

/// Decode a signed 16-bit integer from little-endian bytes
fn decode_i16(bytes: &[u8]) -> Option<i16> {
    bytes
        .get(0..2)
        .and_then(|b| b.try_into().ok())
        .map(i16::from_le_bytes)
}

/// Helper to extract required field with better error message
fn require_field<T>(value: Option<T>, field_name: &str, tag: u8) -> ParseResult<T> {
    value.ok_or_else(|| {
        ParseError::TlvError(format!(
            "Missing required field {} (tag 0x{:02X})",
            field_name, tag
        ))
    })
}

/// A parsed PASSING message
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PassingMessage {
    /// Sequential passing number
    pub passing_number: u32,

    /// Transponder ID (or gate beacon ID)
    pub transponder_id: u32,

    /// RTC time in microseconds since Unix epoch
    pub rtc_time_us: u64,

    /// GPS UTC time (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utc_time_us: Option<u64>,

    /// Signal strength (rider only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strength: Option<u16>,

    /// Number of hits (rider only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hits: Option<u16>,

    /// Transponder string ID (rider only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transponder_string: Option<String>,

    /// Flags
    pub flags: u16,

    /// Decoder ID (hex string showing bytes in wire order)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decoder_id: Option<String>,
}

/// A parsed STATUS message
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusMessage {
    /// Background noise level
    pub noise: u16,

    /// GPS status (0=no fix, 1=locked)
    pub gps_status: u8,

    /// Temperature in tenths of degrees Celsius
    pub temperature: i16,

    /// Number of GPS satellites in use
    pub satellites: u8,

    /// Decoder ID (hex string showing bytes in wire order)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decoder_id: Option<String>,
}

/// A parsed VERSION message
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VersionMessage {
    /// Decoder ID / Registration number (hex string showing bytes in wire order)
    pub decoder_id: String,

    /// Device description
    pub description: String,

    /// Firmware version
    pub version: String,

    /// Build number
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<u16>,
}

impl PassingMessage {
    /// Parse a PASSING message from TLV fields
    pub fn from_tlv_fields(fields: &[TlvField]) -> ParseResult<Self> {
        let mut passing_number = None;
        let mut transponder_id = None;
        let mut rtc_time_us = None;
        let mut utc_time_us = None;
        let mut strength = None;
        let mut hits = None;
        let mut transponder_string = None;
        let mut flags = None;
        let mut decoder_id = None;

        for field in fields {
            match field.tag {
                passing::PASSING_NUMBER => passing_number = TlvDecoder::decode_u32(&field.value),
                passing::TRANSPONDER => transponder_id = TlvDecoder::decode_u32(&field.value),
                passing::RTC_TIME => rtc_time_us = TlvDecoder::decode_u64(&field.value),
                passing::UTC_TIME => utc_time_us = TlvDecoder::decode_u64(&field.value),
                passing::STRENGTH => strength = TlvDecoder::decode_u16(&field.value),
                passing::HITS => hits = TlvDecoder::decode_u16(&field.value),
                passing::STRING => transponder_string = String::from_utf8(field.value.clone()).ok(),
                passing::FLAGS => flags = TlvDecoder::decode_u16(&field.value),
                passing::DECODER_ID => decoder_id = format_decoder_id_u32(&field.value),
                _ => {} // Unknown field, skip
            }
        }

        Ok(PassingMessage {
            passing_number: require_field(
                passing_number,
                "PASSING_NUMBER",
                passing::PASSING_NUMBER,
            )?,
            transponder_id: require_field(transponder_id, "TRANSPONDER", passing::TRANSPONDER)?,
            rtc_time_us: require_field(rtc_time_us, "RTC_TIME", passing::RTC_TIME)?,
            utc_time_us,
            strength,
            hits,
            transponder_string,
            flags: require_field(flags, "FLAGS", passing::FLAGS)?,
            decoder_id,
        })
    }
}

impl StatusMessage {
    /// Parse a STATUS message from TLV fields
    pub fn from_tlv_fields(fields: &[TlvField]) -> ParseResult<Self> {
        let mut noise = None;
        let mut gps_status = None;
        let mut temperature = None;
        let mut satellites = None;
        let mut decoder_id = None;

        for field in fields {
            match field.tag {
                status::NOISE => noise = TlvDecoder::decode_u16(&field.value),
                status::GPS_STATUS => gps_status = field.value.first().copied(),
                status::TEMPERATURE => temperature = decode_i16(&field.value),
                status::SATINUSE => satellites = field.value.first().copied(),
                status::DECODER_ID => decoder_id = format_decoder_id_u32(&field.value),
                _ => {} // Unknown field, skip
            }
        }

        Ok(StatusMessage {
            noise: require_field(noise, "NOISE", status::NOISE)?,
            gps_status: require_field(gps_status, "GPS_STATUS", status::GPS_STATUS)?,
            temperature: require_field(temperature, "TEMPERATURE", status::TEMPERATURE)?,
            satellites: require_field(satellites, "SATINUSE", status::SATINUSE)?,
            decoder_id,
        })
    }
}

impl VersionMessage {
    /// Parse a VERSION message from TLV fields
    pub fn from_tlv_fields(fields: &[TlvField]) -> ParseResult<Self> {
        let mut decoder_id = None;
        let mut description = None;
        let mut version_str = None;
        let mut build = None;

        for field in fields {
            match field.tag {
                version::DECODER_ID => decoder_id = format_decoder_id_u64(&field.value),
                version::DESCRIPTION => description = String::from_utf8(field.value.clone()).ok(),
                version::VERSION => version_str = String::from_utf8(field.value.clone()).ok(),
                version::BUILD => build = TlvDecoder::decode_u16(&field.value),
                _ => {} // Unknown field, skip
            }
        }

        Ok(VersionMessage {
            decoder_id: require_field(decoder_id, "DECODER_ID", version::DECODER_ID)?,
            description: require_field(description, "DESCRIPTION", version::DESCRIPTION)?,
            version: require_field(version_str, "VERSION", version::VERSION)?,
            build,
        })
    }
}

/// Any parsed P3 message
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "message_type")]
pub enum Message {
    #[serde(rename = "PASSING")]
    Passing(PassingMessage),

    #[serde(rename = "STATUS")]
    Status(StatusMessage),

    #[serde(rename = "VERSION")]
    Version(VersionMessage),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_serialization() {
        let msg = Message::Status(StatusMessage {
            noise: 53,
            gps_status: 1,
            temperature: 16,
            satellites: 0,
            decoder_id: Some("D0000C00".to_string()),
        });

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"message_type\":\"STATUS\""));
        assert!(json.contains("\"noise\":53"));
        assert!(json.contains("\"D0000C00\""));
    }
}
