import type { RaceEventMessage, RiderPosition, StagedRider, FinishResult } from '$lib/api/types';

let phase = $state<string>('idle');
let motoId = $state<string | null>(null);
let className = $state<string | null>(null);
let roundType = $state<string | null>(null);
let riders = $state<StagedRider[]>([]);
let positions = $state<RiderPosition[]>([]);
let gateDropTimeUs = $state<number | null>(null);
let finishedCount = $state(0);
let totalRiders = $state(0);
let results = $state<FinishResult[]>([]);
let connected = $state(false);
let socket = $state<WebSocket | null>(null);

function handleMessage(msg: RaceEventMessage) {
	switch (msg.event_type) {
		case 'state_snapshot':
			phase = msg.phase;
			motoId = msg.moto_id;
			className = msg.class_name;
			roundType = msg.round_type;
			riders = msg.riders;
			positions = msg.positions;
			gateDropTimeUs = msg.gate_drop_time_us;
			finishedCount = msg.finished_count;
			totalRiders = msg.total_riders;
			break;

		case 'race_staged':
			phase = 'staged';
			motoId = msg.moto_id;
			className = msg.class_name;
			roundType = msg.round_type;
			riders = msg.riders;
			positions = [];
			results = [];
			gateDropTimeUs = null;
			finishedCount = 0;
			totalRiders = msg.riders.length;
			break;

		case 'gate_drop':
			phase = 'racing';
			gateDropTimeUs = msg.timestamp_us;
			break;

		case 'positions_update':
			positions = msg.positions;
			finishedCount = msg.positions.filter((p) => p.finished).length;
			break;

		case 'rider_finished':
			finishedCount++;
			break;

		case 'race_finished':
			phase = 'finished';
			results = msg.results;
			break;

		case 'race_reset':
			phase = 'idle';
			motoId = null;
			className = null;
			roundType = null;
			riders = [];
			positions = [];
			results = [];
			gateDropTimeUs = null;
			finishedCount = 0;
			totalRiders = 0;
			break;
	}
}

function connect() {
	const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
	const ws = new WebSocket(`${protocol}//${window.location.host}/ws`);

	ws.onopen = () => {
		connected = true;
		socket = ws;
	};

	ws.onclose = () => {
		connected = false;
		socket = null;
		setTimeout(connect, 3000);
	};

	ws.onerror = () => {
		ws.close();
	};

	ws.onmessage = (event) => {
		const data = JSON.parse(event.data);
		// Race events have event_type, P3 messages have message_type
		if (data.event_type) {
			handleMessage(data as RaceEventMessage);
		}
	};
}

function disconnect() {
	socket?.close();
}

export function getRaceStore() {
	return {
		get phase() { return phase; },
		get motoId() { return motoId; },
		get className() { return className; },
		get roundType() { return roundType; },
		get riders() { return riders; },
		get positions() { return positions; },
		get gateDropTimeUs() { return gateDropTimeUs; },
		get finishedCount() { return finishedCount; },
		get totalRiders() { return totalRiders; },
		get results() { return results; },
		get connected() { return connected; },
		connect,
		disconnect
	};
}
