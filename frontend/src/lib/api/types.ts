export interface Track {
	id: string;
	name: string;
	hill_type: '5m' | '8m';
	gate_beacon_id: number;
	location_label: string | null;
	timezone: string | null;
	latitude: number | null;
	longitude: number | null;
	created_at: string;
	updated_at: string;
}

export interface TimingLoop {
	id: string;
	track_id: string;
	name: string;
	decoder_id: string;
	position: number;
	is_finish: boolean;
	is_start: boolean;
	created_at: string;
}

export interface TrackSection {
	id: string;
	track_id: string;
	name: string;
	section_type: string;
	length_m: number;
	position: number;
	loop_id: string | null;
	created_at: string;
}

export interface SectionInput {
	name: string;
	section_type: string;
	length_m: number;
	loop_id: string | null;
}

export interface TrackWithLoops extends Track {
	loops: TimingLoop[];
	sections: TrackSection[];
}

export interface Rider {
	id: string;
	first_name: string;
	last_name: string;
	plate_number: string;
	transponder_id: number;
	transponder_string: string | null;
	age_group: string | null;
	skill_level: 'Novice' | 'Intermediate' | 'Expert' | null;
	gender: 'Male' | 'Female' | null;
	equipment: '20"' | 'Cruiser' | null;
	created_at: string;
	updated_at: string;
}

export interface CreateTrackRequest {
	name: string;
	hill_type?: string;
	gate_beacon_id?: number;
	location_label?: string | null;
	timezone?: string | null;
	latitude?: number | null;
	longitude?: number | null;
}

export interface CreateLoopRequest {
	name: string;
	decoder_id: string;
	position: number;
	is_finish?: boolean;
	is_start?: boolean;
}

export interface CreateRiderRequest {
	first_name: string;
	last_name: string;
	plate_number: string;
	transponder_id: number;
	transponder_string?: string | null;
	age_group?: string | null;
	skill_level?: string | null;
	gender?: string | null;
	equipment?: string | null;
}

// --- Event/Class/Moto types ---

export interface RaceEvent {
	id: string;
	name: string;
	date: string;
	track_id: string;
	status: 'setup' | 'active' | 'completed';
	created_at: string;
}

export interface EventClass {
	id: string;
	event_id: string;
	name: string;
	age_group: string | null;
	skill_level: string | null;
	gender: string | null;
	equipment: string | null;
	race_format: string;
	scoring: string;
	created_at: string;
}

export interface EventWithClasses extends RaceEvent {
	classes: ClassWithRiders[];
}

export interface ClassWithRiders extends EventClass {
	riders: Rider[];
}

export interface Moto {
	id: string;
	event_id: string;
	class_id: string;
	round_type: string;
	round_number: number | null;
	sequence: number;
	status: 'pending' | 'staged' | 'racing' | 'finished';
	created_at: string;
}

export interface MotoEntry {
	id: string;
	moto_id: string;
	rider_id: string;
	lane: number;
	finish_position: number | null;
	elapsed_us: number | null;
	points: number | null;
	dnf: boolean;
	dns: boolean;
	created_at: string;
}

export interface MotoWithEntries extends Moto {
	entries: EntryWithRider[];
}

export interface EntryWithRider extends MotoEntry {
	rider: Rider | null;
}

// --- WebSocket P3 message types ---

export interface PassingMessage {
	message_type: 'PASSING';
	passing_number: number;
	transponder_id: number;
	rtc_time_us: number;
	strength?: number;
	hits?: number;
	transponder_string?: string;
	flags: number;
	decoder_id?: string;
}

export interface StatusMessage {
	message_type: 'STATUS';
	noise: number;
	gps_status: number;
	temperature: number;
	satellites: number;
	decoder_id?: string;
}

export interface VersionMessage {
	message_type: 'VERSION';
	decoder_id: string;
	description: string;
	version: string;
	build?: number;
}

export type P3Message = PassingMessage | StatusMessage | VersionMessage;

// --- WebSocket Race Event types ---

export interface StagedRider {
	rider_id: string;
	first_name: string;
	last_name: string;
	plate_number: string;
	transponder_id: number;
	lane: number;
}

export interface RiderPosition {
	rider_id: string;
	plate_number: string;
	first_name: string;
	last_name: string;
	lane: number;
	position: number;
	last_loop: string | null;
	elapsed_us: number | null;
	gap_to_leader_us: number | null;
	finished: boolean;
	dnf: boolean;
}

export interface FinishResult {
	rider_id: string;
	plate_number: string;
	first_name: string;
	last_name: string;
	position: number;
	elapsed_us: number | null;
	gap_to_leader_us: number | null;
	dnf: boolean;
	dns: boolean;
}

export interface RaceStateResponse {
	phase: string;
	snapshot: RaceEventMessage;
}

export interface DiscoveredDecoder {
	decoder_id: string;
	passing_count: number;
	status_count: number;
	version_count: number;
	gate_hits: number;
	last_seen: string;
	mapped_loop_id: string | null;
	mapped_loop_name: string | null;
	mapped_role: 'start' | 'split' | 'finish' | null;
}

export interface ObservedGateBeacon {
	transponder_id: number;
	hits: number;
}

export interface TrackOnboardingDiscoveryResponse {
	track_id: string;
	window_seconds: number;
	sampled_messages: number;
	decoders: DiscoveredDecoder[];
	gate_beacons: ObservedGateBeacon[];
	generated_at: string;
}

// Race event WebSocket messages
export type RaceEventMessage =
	| { event_type: 'race_staged'; moto_id: string; class_name: string; round_type: string; riders: StagedRider[] }
	| { event_type: 'gate_drop'; moto_id: string; timestamp_us: number }
	| { event_type: 'split_time'; moto_id: string; rider_id: string; loop_name: string; is_finish: boolean; elapsed_us: number; position: number; gap_to_leader_us: number | null }
	| { event_type: 'positions_update'; moto_id: string; positions: RiderPosition[] }
	| { event_type: 'rider_finished'; moto_id: string; rider_id: string; finish_position: number; elapsed_us: number; gap_to_leader_us: number | null }
	| { event_type: 'race_finished'; moto_id: string; results: FinishResult[] }
	| { event_type: 'race_reset' }
	| { event_type: 'state_snapshot'; phase: string; moto_id: string | null; class_name: string | null; round_type: string | null; riders: StagedRider[]; positions: RiderPosition[]; gate_drop_time_us: number | null; finished_count: number; total_riders: number };
