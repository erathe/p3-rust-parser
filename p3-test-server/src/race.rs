//! Auto-race engine: simulate one BMX heat.
//!
//! Model (matching live capture data): all riders launch together on a
//! single gate-drop beacon passing, ride one lap, and cross the finish loop
//! once each — staggered. The lead pack arrives tightly grouped, with gaps
//! widening toward the back of the field. There are no per-rider start
//! passings and no lap counting.

use crate::config::{Rider, Settings};
use crate::simulator::DecoderSimulator;
use rand::Rng;
use rand::seq::SliceRandom;
use std::time::Duration;
use tracing::{error, info};

/// One rider's planned finish-line crossing
#[derive(Debug, Clone)]
pub struct PlannedFinish {
    pub rider: Rider,
    /// Time after the gate drop when this rider crosses the finish loop
    pub after_gate: Duration,
    pub strength: u16,
    pub hits: u16,
}

/// Plan a heat for the enabled riders.
///
/// Finishing order is shuffled per heat. The winner finishes at
/// `race_winner_finish_s`; each subsequent rider trails by a random gap in
/// `[race_gap_min_s, race_gap_max_s]`, scaled up the further back they are.
/// Strength and hits are drawn from live-capture ranges (76-133, 2-33).
pub fn plan_race<R: Rng>(riders: &[Rider], settings: &Settings, rng: &mut R) -> Vec<PlannedFinish> {
    let mut order: Vec<&Rider> = riders.iter().filter(|r| r.enabled).collect();
    order.shuffle(rng);

    let mut finish_s = settings.race_winner_finish_s;
    order
        .into_iter()
        .enumerate()
        .map(|(position, rider)| {
            if position > 0 {
                let gap = rng.gen_range(settings.race_gap_min_s..=settings.race_gap_max_s);
                let widening = 1.0 + (position as f64 - 1.0) * 0.5;
                finish_s += gap * widening;
            }
            PlannedFinish {
                rider: rider.clone(),
                after_gate: Duration::from_secs_f64(finish_s),
                strength: rng.gen_range(76..=133),
                hits: rng.gen_range(2..=33),
            }
        })
        .collect()
}

/// Run one heat: gate drop now, then each rider crosses at the planned time.
///
/// Send failures are logged, not fatal — the race continues so remaining
/// riders still finish. Returns when the last rider has crossed.
pub async fn run_race(sim: DecoderSimulator, riders: Vec<Rider>, settings: Settings) {
    let plan = plan_race(&riders, &settings, &mut rand::thread_rng());
    if plan.is_empty() {
        info!("Race requested but no riders are enabled");
        return;
    }

    info!(
        "Gate drop (beacon {}), {} riders on track",
        settings.gate_transponder,
        plan.len()
    );
    if let Err(e) = sim.send_gate_passing(settings.gate_transponder).await {
        error!("Failed to send gate passing: {}", e);
        return;
    }

    let gate_time = tokio::time::Instant::now();
    for finish in plan {
        tokio::time::sleep_until(gate_time + finish.after_gate).await;

        let string = match finish.rider.string_bytes() {
            Ok(s) => s,
            Err(e) => {
                error!("Skipping rider in slot {}: {}", finish.rider.slot, e);
                continue;
            }
        };

        info!(
            "Finish: {} (+{:.2}s)",
            finish.rider.string_id,
            finish.after_gate.as_secs_f64()
        );
        if let Err(e) = sim
            .send_rider_passing(
                finish.rider.transponder_id,
                &string,
                finish.strength,
                finish.hits,
            )
            .await
        {
            error!("Failed to send rider passing: {}", e);
        }
    }

    info!("Heat complete");
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn roster() -> Vec<Rider> {
        (1..=4)
            .map(|slot| Rider {
                slot,
                transponder_id: 100000000 + slot as u32,
                string_id: format!("FL-0000{}", slot),
                enabled: true,
            })
            .collect()
    }

    fn settings() -> Settings {
        Settings {
            race_winner_finish_s: 40.0,
            race_gap_min_s: 0.2,
            race_gap_max_s: 1.5,
            ..Settings::default()
        }
    }

    #[test]
    fn plan_includes_all_enabled_riders_once() {
        let riders = roster();
        let plan = plan_race(&riders, &settings(), &mut StdRng::seed_from_u64(42));

        assert_eq!(plan.len(), 4);
        let mut planned: Vec<u32> = plan.iter().map(|f| f.rider.transponder_id).collect();
        planned.sort();
        let mut expected: Vec<u32> = riders.iter().map(|r| r.transponder_id).collect();
        expected.sort();
        assert_eq!(planned, expected);
    }

    #[test]
    fn plan_excludes_disabled_riders() {
        let mut riders = roster();
        riders[2].enabled = false;
        let plan = plan_race(&riders, &settings(), &mut StdRng::seed_from_u64(42));

        assert_eq!(plan.len(), 3);
        assert!(
            plan.iter()
                .all(|f| f.rider.transponder_id != riders[2].transponder_id)
        );
    }

    #[test]
    fn finishes_are_staggered_with_bounded_gaps() {
        let cfg = settings();
        for seed in 0..50 {
            let plan = plan_race(&roster(), &cfg, &mut StdRng::seed_from_u64(seed));

            // Winner crosses at the configured time
            assert_eq!(plan[0].after_gate.as_secs_f64(), cfg.race_winner_finish_s);

            // Each subsequent gap within [min, max * widening] for that position
            for i in 1..plan.len() {
                let gap = (plan[i].after_gate - plan[i - 1].after_gate).as_secs_f64();
                let widening = 1.0 + (i as f64 - 1.0) * 0.5;
                assert!(
                    gap >= cfg.race_gap_min_s && gap <= cfg.race_gap_max_s * widening + 1e-9,
                    "seed {}: gap {} at position {} out of bounds",
                    seed,
                    gap,
                    i
                );
            }
        }
    }

    #[test]
    fn signal_values_match_live_capture_ranges() {
        for seed in 0..50 {
            let plan = plan_race(&roster(), &settings(), &mut StdRng::seed_from_u64(seed));
            for finish in &plan {
                assert!((76..=133).contains(&finish.strength));
                assert!((2..=33).contains(&finish.hits));
            }
        }
    }

    #[test]
    fn finishing_order_varies_between_heats() {
        let riders = roster();
        let orders: Vec<Vec<u32>> = (0..20)
            .map(|seed| {
                plan_race(&riders, &settings(), &mut StdRng::seed_from_u64(seed))
                    .iter()
                    .map(|f| f.rider.transponder_id)
                    .collect()
            })
            .collect();
        assert!(
            orders.windows(2).any(|w| w[0] != w[1]),
            "shuffle should produce different finishing orders"
        );
    }

    #[test]
    fn empty_roster_yields_empty_plan() {
        let plan = plan_race(&[], &settings(), &mut StdRng::seed_from_u64(1));
        assert!(plan.is_empty());
    }
}
