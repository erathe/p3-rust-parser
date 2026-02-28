import type {
	FinishResult,
	LiveErrorPayload,
	RaceLiveEnvelope,
	RaceLivePayload,
	RiderPosition,
	StagedRider
} from '$lib/api/types';

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
let selectedTrackId = $state<string | null>(null);
let lastError = $state<LiveErrorPayload | null>(null);
let reconnectTimer = $state<ReturnType<typeof setTimeout> | null>(null);
let connectionToken = $state(0);

const RECONNECT_DELAY_MS = 3000;

interface RaceStore {
	readonly phase: string;
	readonly motoId: string | null;
	readonly className: string | null;
	readonly roundType: string | null;
	readonly riders: StagedRider[];
	readonly positions: RiderPosition[];
	readonly gateDropTimeUs: number | null;
	readonly finishedCount: number;
	readonly totalRiders: number;
	readonly results: FinishResult[];
	readonly connected: boolean;
	readonly selectedTrackId: string | null;
	readonly lastError: LiveErrorPayload | null;
	connect: (trackId?: string) => void;
	disconnect: () => void;
}


function clearReconnectTimer() {
	if (reconnectTimer) {
		clearTimeout(reconnectTimer);
		reconnectTimer = null;
	}
}

function resetRaceState() {
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
}

function handleRacePayload(payload: RaceLivePayload) {
	switch (payload.kind) {
		case 'state_snapshot':
			phase = payload.phase;
			motoId = payload.moto_id;
			className = payload.class_name;
			roundType = payload.round_type;
			riders = payload.riders;
			positions = payload.positions;
			gateDropTimeUs = payload.gate_drop_time_us;
			finishedCount = payload.finished_count;
			totalRiders = payload.total_riders;
			break;

		case 'race_staged':
			phase = 'staged';
			motoId = payload.moto_id;
			className = payload.class_name;
			roundType = payload.round_type;
			riders = payload.riders;
			positions = [];
			results = [];
			gateDropTimeUs = null;
			finishedCount = 0;
			totalRiders = payload.riders.length;
			break;

		case 'gate_drop':
			phase = 'racing';
			gateDropTimeUs = payload.timestamp_us;
			break;

		case 'positions_update':
			positions = payload.positions;
			finishedCount = payload.positions.filter((position) => position.finished).length;
			break;

		case 'rider_finished':
			finishedCount += 1;
			break;

		case 'race_finished':
			phase = 'finished';
			results = payload.results;
			break;

		case 'race_reset':
			resetRaceState();
			break;

		case 'split_time':
			break;
	}
}

function scheduleReconnect(token: number) {
	if (!selectedTrackId || token !== connectionToken) {
		return;
	}

	clearReconnectTimer();
	reconnectTimer = setTimeout(() => {
		if (!selectedTrackId || token !== connectionToken) {
			return;
		}
		connect(selectedTrackId);
	}, RECONNECT_DELAY_MS);
}

function connect(trackId?: string) {
	const nextTrackId = trackId?.trim() ?? '';
	if (!nextTrackId) {
		disconnect();
		return;
	}

	if (selectedTrackId !== nextTrackId) {
		resetRaceState();
	}

	selectedTrackId = nextTrackId;
	clearReconnectTimer();
	connectionToken += 1;
	const token = connectionToken;

	if (socket) {
		socket.close();
		socket = null;
	}

	const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
	const params = new URLSearchParams({
		track_id: nextTrackId,
		channels: 'race',
		from: 'now'
	});
	const ws = new WebSocket(`${protocol}//${window.location.host}/ws/v1/live?${params.toString()}`);

	ws.onopen = () => {
		if (token !== connectionToken) {
			ws.close();
			return;
		}
		connected = true;
		socket = ws;
		lastError = null;
	};

	ws.onclose = () => {
		if (token !== connectionToken) {
			return;
		}
		connected = false;
		socket = null;
		scheduleReconnect(token);
	};

	ws.onerror = () => {
		if (token === connectionToken) {
			lastError = {
				code: 'websocket_error',
				message: 'Race WebSocket connection failed',
				channel: 'race'
			};
		}
		ws.close();
	};

	ws.onmessage = (event) => {
		if (token !== connectionToken) {
			return;
		}

		let envelope: RaceLiveEnvelope;
		try {
			envelope = JSON.parse(event.data) as RaceLiveEnvelope;
		} catch {
			lastError = {
				code: 'invalid_json',
				message: 'Failed to parse race live message',
				channel: 'race'
			};
			return;
		}

		switch (envelope.kind) {
			case 'snapshot':
			case 'event':
				if (envelope.channel === 'race') {
					handleRacePayload(envelope.payload);
					lastError = null;
				}
				break;
			case 'heartbeat':
				break;
			case 'error':
				lastError = envelope.payload;
				break;
		}
	};
}

function disconnect() {
	selectedTrackId = null;
	socket?.close();
	socket = null;
	connected = false;
	clearReconnectTimer();
	connectionToken += 1;
}

export function getRaceStore(): RaceStore {
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
		get selectedTrackId() { return selectedTrackId; },
		get lastError() { return lastError; },
		connect,
		disconnect
	};
}
