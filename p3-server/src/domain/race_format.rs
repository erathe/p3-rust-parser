use serde::{Deserialize, Serialize};

/// Determines the race format based on number of riders in a class.
///
/// BMX race format rules:
/// - 3-8 riders → 3 motos only (total points determine winner)
/// - 9-16 riders → 3 motos + 1 Main (top 8 by points advance to main)
/// - 17-23 riders → 3 motos + 2 Semis + Main (top 4 from each semi to main)
/// - 24-31 riders → 3 motos + Quarters + Semis + Main
/// - 32+ riders → would need multiple rounds of quarters (rare in BMX)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RaceFormat {
    /// 3 motos only, total points scoring
    MotosOnly,
    /// 3 motos + 1 main event
    MotosMain,
    /// 3 motos + 2 semis + 1 main
    MotosSemisMain,
    /// 3 motos + quarters + semis + main
    MotosQuartersSemisMain,
}

impl RaceFormat {
    pub fn as_str(&self) -> &str {
        match self {
            RaceFormat::MotosOnly => "motos_only",
            RaceFormat::MotosMain => "motos_main",
            RaceFormat::MotosSemisMain => "motos_semis_main",
            RaceFormat::MotosQuartersSemisMain => "motos_quarters_semis_main",
        }
    }
}

pub fn determine_format(rider_count: usize) -> RaceFormat {
    match rider_count {
        0..=8 => RaceFormat::MotosOnly,
        9..=16 => RaceFormat::MotosMain,
        17..=23 => RaceFormat::MotosSemisMain,
        _ => RaceFormat::MotosQuartersSemisMain,
    }
}

/// A generated moto assignment.
#[derive(Debug, Clone)]
pub struct MotoAssignment {
    pub round_type: String,
    pub round_number: Option<i64>,
    pub sequence: i64,
    /// (rider_id, lane) pairs
    pub entries: Vec<(String, i64)>,
}

/// Generate moto sheets for the qualifying rounds (motos 1-3).
///
/// Riders are split across heats (max 8 per gate) with lane rotation
/// across the 3 rounds so each rider gets different gate positions.
///
/// Lane rotation pattern:
/// - Moto 1: lanes assigned in order
/// - Moto 2: rotate by +2 positions
/// - Moto 3: rotate by +4 positions
pub fn generate_qualifying_motos(rider_ids: &[String]) -> Vec<MotoAssignment> {
    let rider_count = rider_ids.len();
    if rider_count == 0 {
        return vec![];
    }

    // Determine number of heats per round
    let heats_per_round = (rider_count + 7) / 8; // ceiling division

    let mut motos = Vec::new();
    let mut sequence: i64 = 1;

    for round in 0..3 {
        let round_type = format!("moto{}", round + 1);

        for heat in 0..heats_per_round {
            let mut entries = Vec::new();

            for lane_idx in 0..8 {
                // Determine which rider index this lane maps to for this heat
                let rider_idx = heat * 8 + lane_idx;
                if rider_idx >= rider_count {
                    break;
                }

                // Apply lane rotation: rotate lane assignment based on round
                let rotation = (round * 2) % 8;
                let rotated_lane = ((lane_idx + rotation) % 8) as i64 + 1;

                entries.push((rider_ids[rider_idx].clone(), rotated_lane));
            }

            // Sort entries by lane for consistent ordering
            entries.sort_by_key(|(_, lane)| *lane);

            motos.push(MotoAssignment {
                round_type: round_type.clone(),
                round_number: Some(heat as i64 + 1),
                sequence,
                entries,
            });

            sequence += 1;
        }
    }

    motos
}

/// Generate elimination round motos (semis, quarters, main).
/// These are empty shells — riders are assigned after qualifying results.
pub fn generate_elimination_motos(
    format: &RaceFormat,
    start_sequence: i64,
) -> Vec<MotoAssignment> {
    let mut motos = Vec::new();
    let mut seq = start_sequence;

    match format {
        RaceFormat::MotosOnly => {} // No elimination rounds
        RaceFormat::MotosMain => {
            motos.push(MotoAssignment {
                round_type: "main".into(),
                round_number: None,
                sequence: seq,
                entries: vec![],
            });
        }
        RaceFormat::MotosSemisMain => {
            for i in 1..=2 {
                motos.push(MotoAssignment {
                    round_type: "semi".into(),
                    round_number: Some(i),
                    sequence: seq,
                    entries: vec![],
                });
                seq += 1;
            }
            motos.push(MotoAssignment {
                round_type: "main".into(),
                round_number: None,
                sequence: seq,
                entries: vec![],
            });
        }
        RaceFormat::MotosQuartersSemisMain => {
            // 4 quarter finals
            for i in 1..=4 {
                motos.push(MotoAssignment {
                    round_type: "quarter".into(),
                    round_number: Some(i),
                    sequence: seq,
                    entries: vec![],
                });
                seq += 1;
            }
            // 2 semis
            for i in 1..=2 {
                motos.push(MotoAssignment {
                    round_type: "semi".into(),
                    round_number: Some(i),
                    sequence: seq,
                    entries: vec![],
                });
                seq += 1;
            }
            motos.push(MotoAssignment {
                round_type: "main".into(),
                round_number: None,
                sequence: seq,
                entries: vec![],
            });
        }
    }

    motos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_determination() {
        assert_eq!(determine_format(3), RaceFormat::MotosOnly);
        assert_eq!(determine_format(8), RaceFormat::MotosOnly);
        assert_eq!(determine_format(9), RaceFormat::MotosMain);
        assert_eq!(determine_format(16), RaceFormat::MotosMain);
        assert_eq!(determine_format(17), RaceFormat::MotosSemisMain);
        assert_eq!(determine_format(23), RaceFormat::MotosSemisMain);
        assert_eq!(determine_format(24), RaceFormat::MotosQuartersSemisMain);
        assert_eq!(determine_format(31), RaceFormat::MotosQuartersSemisMain);
    }

    #[test]
    fn test_qualifying_motos_small_class() {
        // 5 riders → 1 heat per round × 3 rounds = 3 motos
        let riders: Vec<String> = (1..=5).map(|i| format!("rider-{i}")).collect();
        let motos = generate_qualifying_motos(&riders);

        assert_eq!(motos.len(), 3);
        assert_eq!(motos[0].round_type, "moto1");
        assert_eq!(motos[1].round_type, "moto2");
        assert_eq!(motos[2].round_type, "moto3");

        // Each moto has all 5 riders
        for moto in &motos {
            assert_eq!(moto.entries.len(), 5);
        }
    }

    #[test]
    fn test_qualifying_motos_full_gate() {
        // 8 riders → 1 heat per round × 3 rounds = 3 motos
        let riders: Vec<String> = (1..=8).map(|i| format!("rider-{i}")).collect();
        let motos = generate_qualifying_motos(&riders);

        assert_eq!(motos.len(), 3);
        for moto in &motos {
            assert_eq!(moto.entries.len(), 8);
        }
    }

    #[test]
    fn test_qualifying_motos_two_heats() {
        // 12 riders → 2 heats per round × 3 rounds = 6 motos
        let riders: Vec<String> = (1..=12).map(|i| format!("rider-{i}")).collect();
        let motos = generate_qualifying_motos(&riders);

        assert_eq!(motos.len(), 6);
        // First heat has 8, second has 4
        assert_eq!(motos[0].entries.len(), 8);
        assert_eq!(motos[1].entries.len(), 4);
    }

    #[test]
    fn test_lane_rotation_across_rounds() {
        // 4 riders, single heat — check that lanes rotate
        let riders: Vec<String> = (1..=4).map(|i| format!("rider-{i}")).collect();
        let motos = generate_qualifying_motos(&riders);

        // Get rider-1's lane in each round
        let lane_r1_m1 = motos[0].entries.iter().find(|(id, _)| id == "rider-1").unwrap().1;
        let lane_r1_m2 = motos[1].entries.iter().find(|(id, _)| id == "rider-1").unwrap().1;
        let lane_r1_m3 = motos[2].entries.iter().find(|(id, _)| id == "rider-1").unwrap().1;

        // Lanes should be different across rounds
        assert_ne!(lane_r1_m1, lane_r1_m2);
        assert_ne!(lane_r1_m2, lane_r1_m3);
    }

    #[test]
    fn test_elimination_motos_main_only() {
        let motos = generate_elimination_motos(&RaceFormat::MotosMain, 4);
        assert_eq!(motos.len(), 1);
        assert_eq!(motos[0].round_type, "main");
        assert_eq!(motos[0].sequence, 4);
    }

    #[test]
    fn test_elimination_motos_semis_main() {
        let motos = generate_elimination_motos(&RaceFormat::MotosSemisMain, 7);
        assert_eq!(motos.len(), 3); // 2 semis + 1 main
        assert_eq!(motos[0].round_type, "semi");
        assert_eq!(motos[1].round_type, "semi");
        assert_eq!(motos[2].round_type, "main");
    }

    #[test]
    fn test_elimination_motos_quarters_semis_main() {
        let motos = generate_elimination_motos(&RaceFormat::MotosQuartersSemisMain, 10);
        assert_eq!(motos.len(), 7); // 4 quarters + 2 semis + 1 main
        assert_eq!(motos[0].round_type, "quarter");
        assert_eq!(motos[3].round_type, "quarter");
        assert_eq!(motos[4].round_type, "semi");
        assert_eq!(motos[5].round_type, "semi");
        assert_eq!(motos[6].round_type, "main");
    }

    #[test]
    fn test_sequences_are_monotonic() {
        let riders: Vec<String> = (1..=12).map(|i| format!("rider-{i}")).collect();
        let qualifying = generate_qualifying_motos(&riders);
        let last_seq = qualifying.last().unwrap().sequence;
        let elimination = generate_elimination_motos(&RaceFormat::MotosMain, last_seq + 1);

        let all_sequences: Vec<i64> = qualifying
            .iter()
            .chain(elimination.iter())
            .map(|m| m.sequence)
            .collect();

        for window in all_sequences.windows(2) {
            assert!(window[1] > window[0], "Sequences must be strictly increasing");
        }
    }
}
