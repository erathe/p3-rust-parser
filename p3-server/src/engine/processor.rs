use std::collections::HashMap;

use p3_parser::messages::PassingMessage;
use p3_protocol::fields::reserved_ids;

use crate::domain::race_event::{LoopConfig, RiderState, TrackConfig};

/// Check if a passing message is a gate drop signal.
///
/// A gate drop is identified by:
/// 1. The transponder ID matching the track's configured gate beacon ID, OR
/// 2. The transponder ID being any reserved system ID (9991, 9992, 9995)
pub fn is_gate_drop(passing: &PassingMessage, track: &TrackConfig) -> bool {
    passing.transponder_id == track.gate_beacon_id || reserved_ids::is_reserved(passing.transponder_id)
}

/// Calculate a rider's position at a specific loop.
/// Position is determined by comparing elapsed times at this loop across all riders
/// who have reached it.
pub fn calculate_position_at_loop(
    riders: &HashMap<u32, RiderState>,
    loop_config: &LoopConfig,
    current_rider_id: &str,
) -> u32 {
    let current_time = riders
        .values()
        .find(|r| r.rider_id == current_rider_id)
        .and_then(|r| r.splits.get(&loop_config.loop_id))
        .copied();

    let current_time = match current_time {
        Some(t) => t,
        None => return 1,
    };

    // Count how many riders have a faster time at this loop
    let faster_count = riders
        .values()
        .filter(|r| r.rider_id != current_rider_id)
        .filter_map(|r| r.splits.get(&loop_config.loop_id))
        .filter(|&&time| time < current_time)
        .count();

    (faster_count + 1) as u32
}

/// Find the leader's (fastest) time at a specific loop.
pub fn leader_time_at_loop(
    riders: &HashMap<u32, RiderState>,
    loop_config: &LoopConfig,
) -> Option<u64> {
    riders
        .values()
        .filter_map(|r| r.splits.get(&loop_config.loop_id))
        .copied()
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_track(gate_beacon_id: u32) -> TrackConfig {
        TrackConfig {
            track_id: "t1".into(),
            name: "Test".into(),
            gate_beacon_id,
            loops: vec![],
        }
    }

    fn make_passing(transponder_id: u32) -> PassingMessage {
        PassingMessage {
            passing_number: 1,
            transponder_id,
            rtc_time_us: 1_000_000,
            utc_time_us: None,
            strength: None,
            hits: None,
            transponder_string: None,
            flags: 0,
            decoder_id: None,
        }
    }

    #[test]
    fn test_gate_drop_configured_beacon() {
        let track = make_track(9992);
        assert!(is_gate_drop(&make_passing(9992), &track));
    }

    #[test]
    fn test_gate_drop_all_reserved_ids() {
        let track = make_track(9992);
        assert!(is_gate_drop(&make_passing(9991), &track));
        assert!(is_gate_drop(&make_passing(9992), &track));
        assert!(is_gate_drop(&make_passing(9995), &track));
    }

    #[test]
    fn test_rider_not_gate_drop() {
        let track = make_track(9992);
        assert!(!is_gate_drop(&make_passing(1001), &track));
        assert!(!is_gate_drop(&make_passing(5000), &track));
    }

    #[test]
    fn test_position_calculation() {
        let loop_config = LoopConfig {
            loop_id: "loop-1".into(),
            name: "Corner 1".into(),
            decoder_id: "D001".into(),
            position: 1,
            is_start: false,
            is_finish: false,
        };

        let mut riders = HashMap::new();

        // Rider A: 5.0s at loop-1
        let mut rider_a = RiderState::new("a".into(), "A".into(), "A".into(), "1".into(), 1001, 1);
        rider_a.splits.insert("loop-1".into(), 5_000_000);
        riders.insert(1001, rider_a);

        // Rider B: 4.5s at loop-1 (fastest)
        let mut rider_b = RiderState::new("b".into(), "B".into(), "B".into(), "2".into(), 1002, 2);
        rider_b.splits.insert("loop-1".into(), 4_500_000);
        riders.insert(1002, rider_b);

        // Rider C: 5.2s at loop-1
        let mut rider_c = RiderState::new("c".into(), "C".into(), "C".into(), "3".into(), 1003, 3);
        rider_c.splits.insert("loop-1".into(), 5_200_000);
        riders.insert(1003, rider_c);

        assert_eq!(calculate_position_at_loop(&riders, &loop_config, "a"), 2);
        assert_eq!(calculate_position_at_loop(&riders, &loop_config, "b"), 1);
        assert_eq!(calculate_position_at_loop(&riders, &loop_config, "c"), 3);
    }

    #[test]
    fn test_leader_time() {
        let loop_config = LoopConfig {
            loop_id: "loop-1".into(),
            name: "Corner 1".into(),
            decoder_id: "D001".into(),
            position: 1,
            is_start: false,
            is_finish: false,
        };

        let mut riders = HashMap::new();

        let mut rider_a = RiderState::new("a".into(), "A".into(), "A".into(), "1".into(), 1001, 1);
        rider_a.splits.insert("loop-1".into(), 5_000_000);
        riders.insert(1001, rider_a);

        let mut rider_b = RiderState::new("b".into(), "B".into(), "B".into(), "2".into(), 1002, 2);
        rider_b.splits.insert("loop-1".into(), 4_500_000);
        riders.insert(1002, rider_b);

        assert_eq!(leader_time_at_loop(&riders, &loop_config), Some(4_500_000));
    }
}
