use std::collections::HashMap;
use std::sync::Arc;

use p3_parser::messages::PassingMessage;
use tokio::sync::broadcast;
use tracing::{info, warn};

use crate::domain::race_event::{
    FinishResult, LoopConfig, RaceEvent, RiderPosition, RiderState, StagedRider, TrackConfig,
};

use super::processor;

/// The current phase of a race.
#[derive(Debug, Clone)]
pub enum RacePhase {
    /// No race in progress, waiting for operator to stage a moto.
    Idle,

    /// A moto is loaded and riders are on the gate, waiting for gate drop.
    Staged {
        moto_id: String,
        class_name: String,
        round_type: String,
    },

    /// Gate has dropped, race is in progress.
    Racing {
        moto_id: String,
        class_name: String,
        round_type: String,
        gate_drop_time_us: u64,
    },

    /// All riders have finished (or force-finished).
    Finished {
        moto_id: String,
        class_name: String,
        round_type: String,
    },
}

impl RacePhase {
    pub fn name(&self) -> &str {
        match self {
            RacePhase::Idle => "idle",
            RacePhase::Staged { .. } => "staged",
            RacePhase::Racing { .. } => "racing",
            RacePhase::Finished { .. } => "finished",
        }
    }
}

/// The race engine processes P3 passings and produces race events.
pub struct RaceEngine {
    /// Current race phase
    phase: RacePhase,
    /// Current track configuration (timing loops, gate beacon ID)
    track_config: Option<TrackConfig>,
    /// Rider states keyed by transponder_id for fast lookup during racing
    riders_by_transponder: HashMap<u32, RiderState>,
    /// Rider states keyed by rider_id for result lookups
    rider_ids: Vec<String>,
    /// Decoder ID → loop config mapping for fast lookup
    decoder_to_loop: HashMap<String, LoopConfig>,
    /// Next finish position to assign
    next_finish_position: u32,
    /// Broadcast channel for race events
    event_tx: broadcast::Sender<Arc<RaceEvent>>,
}

impl RaceEngine {
    pub fn new(event_tx: broadcast::Sender<Arc<RaceEvent>>) -> Self {
        Self {
            phase: RacePhase::Idle,
            track_config: None,
            riders_by_transponder: HashMap::new(),
            rider_ids: Vec::new(),
            decoder_to_loop: HashMap::new(),
            next_finish_position: 1,
            event_tx,
        }
    }

    pub fn phase(&self) -> &RacePhase {
        &self.phase
    }

    /// Set the track configuration for the engine.
    pub fn set_track(&mut self, config: TrackConfig) {
        self.decoder_to_loop.clear();
        for l in &config.loops {
            self.decoder_to_loop.insert(l.decoder_id.clone(), l.clone());
        }
        info!(track = %config.name, loops = config.loops.len(), "Track config loaded");
        self.track_config = Some(config);
    }

    /// Stage a moto: load riders onto the gate, ready for gate drop.
    pub fn stage_moto(
        &mut self,
        moto_id: String,
        class_name: String,
        round_type: String,
        riders: Vec<StagedRider>,
    ) {
        if !matches!(self.phase, RacePhase::Idle | RacePhase::Finished { .. }) {
            warn!(
                current_phase = self.phase.name(),
                "Cannot stage moto: race is in progress"
            );
            return;
        }

        self.riders_by_transponder.clear();
        self.rider_ids.clear();
        self.next_finish_position = 1;

        for rider in &riders {
            self.rider_ids.push(rider.rider_id.clone());
            self.riders_by_transponder.insert(
                rider.transponder_id,
                RiderState::new(
                    rider.rider_id.clone(),
                    rider.first_name.clone(),
                    rider.last_name.clone(),
                    rider.plate_number.clone(),
                    rider.transponder_id,
                    rider.lane,
                ),
            );
        }

        info!(
            moto_id = %moto_id,
            class = %class_name,
            round = %round_type,
            riders = riders.len(),
            "Moto staged"
        );

        self.phase = RacePhase::Staged {
            moto_id: moto_id.clone(),
            class_name: class_name.clone(),
            round_type: round_type.clone(),
        };

        self.broadcast(RaceEvent::RaceStaged {
            moto_id,
            class_name,
            round_type,
            riders,
        });
    }

    /// Process an incoming P3 passing message.
    /// Returns any race events generated.
    pub fn process_passing(&mut self, passing: &PassingMessage) -> Vec<RaceEvent> {
        let track = match &self.track_config {
            Some(t) => t,
            None => return vec![],
        };

        match &self.phase {
            RacePhase::Idle => vec![],

            RacePhase::Staged {
                moto_id,
                class_name,
                round_type,
            } => {
                // Only thing we're looking for is a gate drop
                if processor::is_gate_drop(passing, track) {
                    let moto_id = moto_id.clone();
                    let class_name = class_name.clone();
                    let round_type = round_type.clone();

                    self.phase = RacePhase::Racing {
                        moto_id: moto_id.clone(),
                        class_name,
                        round_type,
                        gate_drop_time_us: passing.rtc_time_us,
                    };

                    info!(
                        moto_id = %moto_id,
                        timestamp = passing.rtc_time_us,
                        "Gate drop detected"
                    );

                    let event = RaceEvent::GateDrop {
                        moto_id,
                        timestamp_us: passing.rtc_time_us,
                    };
                    self.broadcast(event.clone());
                    vec![event]
                } else {
                    vec![]
                }
            }

            RacePhase::Racing {
                moto_id,
                class_name,
                round_type,
                gate_drop_time_us,
            } => {
                let moto_id = moto_id.clone();
                let class_name = class_name.clone();
                let round_type = round_type.clone();
                let gate_drop_time_us = *gate_drop_time_us;

                // Ignore gate beacon passings during racing
                if processor::is_gate_drop(passing, track) {
                    return vec![];
                }

                let mut events = vec![];

                // Try to match this passing to a rider and a loop
                if let Some(loop_config) = passing
                    .decoder_id
                    .as_ref()
                    .and_then(|did| self.decoder_to_loop.get(did))
                {
                    let loop_config = loop_config.clone();

                    if let Some(rider) = self.riders_by_transponder.get_mut(&passing.transponder_id)
                    {
                        let elapsed_us = passing.rtc_time_us.saturating_sub(gate_drop_time_us);

                        // Only record if this is a new loop (further along the track)
                        // or if the rider hasn't been seen at this loop yet
                        let dominated = rider
                            .last_loop_position
                            .is_some_and(|last_pos| loop_config.position < last_pos);

                        if dominated && !loop_config.is_finish {
                            // Rider went backwards or duplicate at an earlier loop — ignore
                            return events;
                        }

                        // Record the split
                        rider.splits.insert(loop_config.loop_id.clone(), elapsed_us);
                        rider.last_loop_position = Some(loop_config.position);
                        rider.last_loop_name = Some(loop_config.name.clone());
                        rider.last_elapsed_us = Some(elapsed_us);

                        let rider_id = rider.rider_id.clone();

                        if loop_config.is_finish && !rider.finished {
                            // Rider finished!
                            rider.finished = true;
                            rider.finish_elapsed_us = Some(elapsed_us);
                            let pos = self.next_finish_position;
                            rider.finish_position = Some(pos);
                            self.next_finish_position += 1;

                            // Calculate gap to leader
                            let leader_time = self.leader_finish_time();
                            let gap = leader_time.map(|lt| elapsed_us.saturating_sub(lt));

                            info!(
                                rider = %rider_id,
                                position = pos,
                                elapsed_us = elapsed_us,
                                "Rider finished"
                            );

                            let split_event = RaceEvent::SplitTime {
                                moto_id: moto_id.clone(),
                                rider_id: rider_id.clone(),
                                loop_name: loop_config.name.clone(),
                                is_finish: true,
                                elapsed_us,
                                position: pos,
                                gap_to_leader_us: gap,
                            };
                            events.push(split_event.clone());
                            self.broadcast(split_event);

                            let finish_event = RaceEvent::RiderFinished {
                                moto_id: moto_id.clone(),
                                rider_id: rider_id.clone(),
                                finish_position: pos,
                                elapsed_us,
                                gap_to_leader_us: gap,
                            };
                            events.push(finish_event.clone());
                            self.broadcast(finish_event);
                        } else if !rider.finished {
                            // Split time at a non-finish loop
                            let position = processor::calculate_position_at_loop(
                                &self.riders_by_transponder,
                                &loop_config,
                                &rider_id,
                            );

                            // Gap to leader at this loop
                            let leader_time = processor::leader_time_at_loop(
                                &self.riders_by_transponder,
                                &loop_config,
                            );
                            let gap = leader_time.map(|lt| elapsed_us.saturating_sub(lt));

                            let split_event = RaceEvent::SplitTime {
                                moto_id: moto_id.clone(),
                                rider_id: rider_id.clone(),
                                loop_name: loop_config.name.clone(),
                                is_finish: false,
                                elapsed_us,
                                position,
                                gap_to_leader_us: gap,
                            };
                            events.push(split_event.clone());
                            self.broadcast(split_event);
                        }

                        // Broadcast updated positions
                        let positions = self.calculate_positions();
                        let pos_event = RaceEvent::PositionsUpdate {
                            moto_id: moto_id.clone(),
                            positions,
                        };
                        events.push(pos_event.clone());
                        self.broadcast(pos_event);

                        // Check if all riders have finished
                        let all_finished = self
                            .riders_by_transponder
                            .values()
                            .all(|r| r.finished || r.dnf);

                        if all_finished && !self.riders_by_transponder.is_empty() {
                            let results = self.build_results();
                            info!(moto_id = %moto_id, "Race finished — all riders done");

                            self.phase = RacePhase::Finished {
                                moto_id: moto_id.clone(),
                                class_name,
                                round_type,
                            };

                            let finish_event = RaceEvent::RaceFinished { moto_id, results };
                            events.push(finish_event.clone());
                            self.broadcast(finish_event);
                        }
                    }
                }

                events
            }

            RacePhase::Finished { .. } => vec![],
        }
    }

    /// Force-finish the current race (operator action for timeouts, etc.)
    pub fn force_finish(&mut self) -> Option<RaceEvent> {
        match &self.phase {
            RacePhase::Racing {
                moto_id,
                class_name,
                round_type,
                ..
            } => {
                let moto_id = moto_id.clone();
                let class_name = class_name.clone();
                let round_type = round_type.clone();

                // Mark unfinished riders as DNF
                for rider in self.riders_by_transponder.values_mut() {
                    if !rider.finished {
                        rider.dnf = true;
                    }
                }

                let results = self.build_results();
                info!(moto_id = %moto_id, "Race force-finished by operator");

                self.phase = RacePhase::Finished {
                    moto_id: moto_id.clone(),
                    class_name,
                    round_type,
                };

                let event = RaceEvent::RaceFinished { moto_id, results };
                self.broadcast(event.clone());
                Some(event)
            }
            _ => {
                warn!(phase = self.phase.name(), "Cannot force-finish: not racing");
                None
            }
        }
    }

    /// Reset back to idle.
    pub fn reset(&mut self) {
        info!(phase = self.phase.name(), "Race reset to idle");
        self.phase = RacePhase::Idle;
        self.riders_by_transponder.clear();
        self.rider_ids.clear();
        self.next_finish_position = 1;
        self.broadcast(RaceEvent::RaceReset);
    }

    /// Build a snapshot of the current state for newly connected clients.
    pub fn state_snapshot(&self) -> RaceEvent {
        let (moto_id, class_name, round_type, gate_drop_time_us) = match &self.phase {
            RacePhase::Idle => (None, None, None, None),
            RacePhase::Staged {
                moto_id,
                class_name,
                round_type,
            } => (
                Some(moto_id.clone()),
                Some(class_name.clone()),
                Some(round_type.clone()),
                None,
            ),
            RacePhase::Racing {
                moto_id,
                class_name,
                round_type,
                gate_drop_time_us,
            } => (
                Some(moto_id.clone()),
                Some(class_name.clone()),
                Some(round_type.clone()),
                Some(*gate_drop_time_us),
            ),
            RacePhase::Finished {
                moto_id,
                class_name,
                round_type,
            } => (
                Some(moto_id.clone()),
                Some(class_name.clone()),
                Some(round_type.clone()),
                None,
            ),
        };

        let riders: Vec<StagedRider> = self
            .riders_by_transponder
            .values()
            .map(|r| StagedRider {
                rider_id: r.rider_id.clone(),
                first_name: r.first_name.clone(),
                last_name: r.last_name.clone(),
                plate_number: r.plate_number.clone(),
                transponder_id: r.transponder_id,
                lane: r.lane,
            })
            .collect();

        let finished_count = self
            .riders_by_transponder
            .values()
            .filter(|r| r.finished)
            .count() as u32;

        RaceEvent::StateSnapshot {
            phase: self.phase.name().to_string(),
            moto_id,
            class_name,
            round_type,
            riders,
            positions: self.calculate_positions(),
            gate_drop_time_us,
            finished_count,
            total_riders: self.riders_by_transponder.len() as u32,
        }
    }

    // --- Private helpers ---

    fn broadcast(&self, event: RaceEvent) {
        // Ignore send errors (no subscribers is fine)
        let _ = self.event_tx.send(Arc::new(event));
    }

    fn leader_finish_time(&self) -> Option<u64> {
        self.riders_by_transponder
            .values()
            .filter(|r| r.finished)
            .filter_map(|r| r.finish_elapsed_us)
            .min()
    }

    /// Calculate current race positions based on:
    /// 1. Finished riders ranked by finish position
    /// 2. Unfinished riders ranked by furthest loop reached, then elapsed time
    fn calculate_positions(&self) -> Vec<RiderPosition> {
        let mut finished: Vec<&RiderState> = self
            .riders_by_transponder
            .values()
            .filter(|r| r.finished)
            .collect();
        finished.sort_by_key(|r| r.finish_position);

        let mut racing: Vec<&RiderState> = self
            .riders_by_transponder
            .values()
            .filter(|r| !r.finished && !r.dnf)
            .collect();
        // Sort by: furthest loop (desc), then elapsed time at that loop (asc)
        racing.sort_by(|a, b| {
            let loop_cmp = b
                .last_loop_position
                .unwrap_or(0)
                .cmp(&a.last_loop_position.unwrap_or(0));
            if loop_cmp != std::cmp::Ordering::Equal {
                return loop_cmp;
            }
            a.last_elapsed_us
                .unwrap_or(u64::MAX)
                .cmp(&b.last_elapsed_us.unwrap_or(u64::MAX))
        });

        let mut dnf: Vec<&RiderState> = self
            .riders_by_transponder
            .values()
            .filter(|r| r.dnf)
            .collect();
        dnf.sort_by_key(|r| r.lane);

        let leader_finish = self.leader_finish_time();

        let mut positions = Vec::new();
        let mut pos: u32 = 1;

        for rider in finished.iter().chain(racing.iter()).chain(dnf.iter()) {
            let gap = match (leader_finish, rider.finish_elapsed_us) {
                (Some(lt), Some(ft)) if pos > 1 => Some(ft.saturating_sub(lt)),
                _ => rider.last_elapsed_us.and_then(|elapsed| {
                    // For non-finished riders, gap is less meaningful
                    // but we can show gap to leader at the same loop
                    Some(elapsed).filter(|_| pos > 1)
                }),
            };

            positions.push(rider.to_position(pos, gap));
            pos += 1;
        }

        positions
    }

    fn build_results(&self) -> Vec<FinishResult> {
        let mut results: Vec<FinishResult> = self
            .riders_by_transponder
            .values()
            .map(|r| {
                let leader_time = self.leader_finish_time();
                let gap = match (leader_time, r.finish_elapsed_us) {
                    (Some(lt), Some(ft)) if r.finish_position != Some(1) => {
                        Some(ft.saturating_sub(lt))
                    }
                    _ => None,
                };

                FinishResult {
                    rider_id: r.rider_id.clone(),
                    plate_number: r.plate_number.clone(),
                    first_name: r.first_name.clone(),
                    last_name: r.last_name.clone(),
                    position: r.finish_position.unwrap_or(0),
                    elapsed_us: r.finish_elapsed_us,
                    gap_to_leader_us: gap,
                    dnf: r.dnf,
                    dns: false,
                }
            })
            .collect();

        // Finished riders first (by position), then DNF riders
        results.sort_by(|a, b| match (a.dnf, b.dnf) {
            (false, true) => std::cmp::Ordering::Less,
            (true, false) => std::cmp::Ordering::Greater,
            _ => a.position.cmp(&b.position),
        });

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::race_event::StagedRider;

    fn test_track() -> TrackConfig {
        TrackConfig {
            track_id: "track-1".into(),
            name: "Test BMX Track".into(),
            gate_beacon_id: 9992,
            loops: vec![
                LoopConfig {
                    loop_id: "loop-start".into(),
                    name: "Start Hill".into(),
                    decoder_id: "D0000C01".into(),
                    position: 0,
                    is_start: true,
                    is_finish: false,
                },
                LoopConfig {
                    loop_id: "loop-corner1".into(),
                    name: "Corner 1".into(),
                    decoder_id: "D0000C02".into(),
                    position: 1,
                    is_start: false,
                    is_finish: false,
                },
                LoopConfig {
                    loop_id: "loop-finish".into(),
                    name: "Finish".into(),
                    decoder_id: "D0000C03".into(),
                    position: 2,
                    is_start: false,
                    is_finish: true,
                },
            ],
        }
    }

    fn test_riders() -> Vec<StagedRider> {
        vec![
            StagedRider {
                rider_id: "rider-1".into(),
                first_name: "Alice".into(),
                last_name: "Smith".into(),
                plate_number: "42".into(),
                transponder_id: 1001,
                lane: 1,
            },
            StagedRider {
                rider_id: "rider-2".into(),
                first_name: "Bob".into(),
                last_name: "Jones".into(),
                plate_number: "7".into(),
                transponder_id: 1002,
                lane: 2,
            },
            StagedRider {
                rider_id: "rider-3".into(),
                first_name: "Charlie".into(),
                last_name: "Brown".into(),
                plate_number: "99".into(),
                transponder_id: 1003,
                lane: 3,
            },
        ]
    }

    fn make_passing(transponder_id: u32, decoder_id: &str, rtc_time_us: u64) -> PassingMessage {
        PassingMessage {
            passing_number: 1,
            transponder_id,
            rtc_time_us,
            utc_time_us: None,
            strength: Some(100),
            hits: Some(40),
            transponder_string: None,
            flags: 0,
            decoder_id: Some(decoder_id.to_string()),
        }
    }

    #[test]
    fn test_idle_ignores_passings() {
        let (tx, _rx) = broadcast::channel(64);
        let mut engine = RaceEngine::new(tx);
        engine.set_track(test_track());

        let passing = make_passing(1001, "D0000C01", 1_000_000);
        let events = engine.process_passing(&passing);
        assert!(events.is_empty());
    }

    #[test]
    fn test_stage_moto() {
        let (tx, mut rx) = broadcast::channel(64);
        let mut engine = RaceEngine::new(tx);
        engine.set_track(test_track());

        engine.stage_moto(
            "moto-1".into(),
            "Novice".into(),
            "moto1".into(),
            test_riders(),
        );

        assert!(matches!(engine.phase(), RacePhase::Staged { .. }));

        let event = rx.try_recv().unwrap();
        assert!(matches!(event.as_ref(), RaceEvent::RaceStaged { .. }));
    }

    #[test]
    fn test_gate_drop_transitions_to_racing() {
        let (tx, mut rx) = broadcast::channel(64);
        let mut engine = RaceEngine::new(tx);
        engine.set_track(test_track());
        engine.stage_moto(
            "moto-1".into(),
            "Novice".into(),
            "moto1".into(),
            test_riders(),
        );
        let _ = rx.try_recv(); // consume RaceStaged

        // Gate beacon passing
        let gate = make_passing(9992, "D0000C01", 10_000_000);
        let events = engine.process_passing(&gate);

        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], RaceEvent::GateDrop { .. }));
        assert!(matches!(engine.phase(), RacePhase::Racing { .. }));
    }

    #[test]
    fn test_split_time_and_positions() {
        let (tx, mut rx) = broadcast::channel(64);
        let mut engine = RaceEngine::new(tx);
        engine.set_track(test_track());
        engine.stage_moto(
            "moto-1".into(),
            "Novice".into(),
            "moto1".into(),
            test_riders(),
        );

        // Gate drop at T=10_000_000
        let gate = make_passing(9992, "D0000C01", 10_000_000);
        engine.process_passing(&gate);
        // Drain staged + gate events
        while rx.try_recv().is_ok() {}

        // Rider 2 crosses start hill first (T+1s)
        let p1 = make_passing(1002, "D0000C01", 11_000_000);
        let events = engine.process_passing(&p1);
        // Should get SplitTime + PositionsUpdate
        assert!(
            events.iter().any(
                |e| matches!(e, RaceEvent::SplitTime { rider_id, .. } if rider_id == "rider-2")
            )
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, RaceEvent::PositionsUpdate { .. }))
        );

        // Rider 1 crosses start hill second (T+1.2s)
        let p2 = make_passing(1001, "D0000C01", 11_200_000);
        let events = engine.process_passing(&p2);
        assert!(
            events.iter().any(
                |e| matches!(e, RaceEvent::SplitTime { rider_id, .. } if rider_id == "rider-1")
            )
        );
    }

    #[test]
    fn test_finish_and_race_complete() {
        let (tx, _rx) = broadcast::channel(64);
        let mut engine = RaceEngine::new(tx);
        engine.set_track(test_track());
        engine.stage_moto(
            "moto-1".into(),
            "Novice".into(),
            "moto1".into(),
            test_riders(),
        );

        // Gate drop
        engine.process_passing(&make_passing(9992, "D0000C01", 10_000_000));

        // All three riders cross all loops and finish
        // Rider 1 finishes first
        engine.process_passing(&make_passing(1001, "D0000C01", 11_000_000)); // start
        engine.process_passing(&make_passing(1001, "D0000C02", 15_000_000)); // corner
        let events = engine.process_passing(&make_passing(1001, "D0000C03", 20_000_000)); // finish
        assert!(events.iter().any(|e| matches!(
            e,
            RaceEvent::RiderFinished {
                finish_position: 1,
                ..
            }
        )));

        // Rider 2 finishes second
        engine.process_passing(&make_passing(1002, "D0000C01", 11_200_000));
        engine.process_passing(&make_passing(1002, "D0000C02", 15_500_000));
        let events = engine.process_passing(&make_passing(1002, "D0000C03", 21_000_000));
        assert!(events.iter().any(|e| matches!(
            e,
            RaceEvent::RiderFinished {
                finish_position: 2,
                ..
            }
        )));

        // Rider 3 finishes third → triggers RaceFinished
        engine.process_passing(&make_passing(1003, "D0000C01", 11_500_000));
        engine.process_passing(&make_passing(1003, "D0000C02", 16_000_000));
        let events = engine.process_passing(&make_passing(1003, "D0000C03", 22_000_000));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, RaceEvent::RaceFinished { .. }))
        );
        assert!(matches!(engine.phase(), RacePhase::Finished { .. }));
    }

    #[test]
    fn test_force_finish() {
        let (tx, _rx) = broadcast::channel(64);
        let mut engine = RaceEngine::new(tx);
        engine.set_track(test_track());
        engine.stage_moto(
            "moto-1".into(),
            "Novice".into(),
            "moto1".into(),
            test_riders(),
        );

        // Gate drop
        engine.process_passing(&make_passing(9992, "D0000C01", 10_000_000));

        // Only rider 1 finishes
        engine.process_passing(&make_passing(1001, "D0000C01", 11_000_000));
        engine.process_passing(&make_passing(1001, "D0000C03", 20_000_000));

        // Force finish
        let event = engine.force_finish();
        assert!(event.is_some());
        assert!(matches!(engine.phase(), RacePhase::Finished { .. }));

        if let Some(RaceEvent::RaceFinished { results, .. }) = event {
            // 1 finished + 2 DNF
            assert_eq!(results.len(), 3);
            let dnf_count = results.iter().filter(|r| r.dnf).count();
            assert_eq!(dnf_count, 2);
        }
    }

    #[test]
    fn test_reset_to_idle() {
        let (tx, _rx) = broadcast::channel(64);
        let mut engine = RaceEngine::new(tx);
        engine.set_track(test_track());
        engine.stage_moto(
            "moto-1".into(),
            "Novice".into(),
            "moto1".into(),
            test_riders(),
        );

        engine.reset();
        assert!(matches!(engine.phase(), RacePhase::Idle));
    }

    #[test]
    fn test_state_snapshot() {
        let (tx, _rx) = broadcast::channel(64);
        let mut engine = RaceEngine::new(tx);
        engine.set_track(test_track());
        engine.stage_moto(
            "moto-1".into(),
            "Novice".into(),
            "moto1".into(),
            test_riders(),
        );

        let snapshot = engine.state_snapshot();
        if let RaceEvent::StateSnapshot {
            phase,
            moto_id,
            riders,
            total_riders,
            ..
        } = snapshot
        {
            assert_eq!(phase, "staged");
            assert_eq!(moto_id, Some("moto-1".to_string()));
            assert_eq!(riders.len(), 3);
            assert_eq!(total_riders, 3);
        } else {
            panic!("Expected StateSnapshot");
        }
    }
}
