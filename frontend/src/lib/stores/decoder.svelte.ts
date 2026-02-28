import type {
	DecoderLiveEnvelope,
	DecoderStatusRow,
	LiveErrorPayload,
	P3Message,
	PassingMessage,
	StatusMessage
} from '$lib/api/types';

let connected = $state(false);
let socket = $state<WebSocket | null>(null);
let selectedTrackId = $state<string | null>(null);
let lastStatus = $state<StatusMessage | null>(null);
let recentPassings = $state<PassingMessage[]>([]);
let snapshotRows = $state<DecoderStatusRow[]>([]);
let lastError = $state<LiveErrorPayload | null>(null);
let messageCount = $state(0);
let reconnectTimer = $state<ReturnType<typeof setTimeout> | null>(null);
let connectionToken = $state(0);

const MAX_RECENT = 50;
const RECONNECT_DELAY_MS = 3000;

interface DecoderStore {
	readonly connected: boolean;
	readonly selectedTrackId: string | null;
	readonly lastStatus: StatusMessage | null;
	readonly recentPassings: PassingMessage[];
	readonly snapshotRows: DecoderStatusRow[];
	readonly lastError: LiveErrorPayload | null;
	readonly messageCount: number;
	connect: (trackId: string) => void;
	disconnect: () => void;
}

function clearReconnectTimer() {
	if (reconnectTimer) {
		clearTimeout(reconnectTimer);
		reconnectTimer = null;
	}
}

function resetTelemetry() {
	messageCount = 0;
	recentPassings = [];
	lastStatus = null;
	snapshotRows = [];
	lastError = null;
}

function handleP3Message(message: P3Message) {
	messageCount++;

	if (message.message_type === 'STATUS') {
		lastStatus = message;
	} else if (message.message_type === 'PASSING') {
		recentPassings = [message, ...recentPassings.slice(0, MAX_RECENT - 1)];
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

function connect(trackId: string) {
	const nextTrackId = trackId.trim();
	if (!nextTrackId) {
		disconnect();
		return;
	}

	if (selectedTrackId !== nextTrackId) {
		resetTelemetry();
	}

	selectedTrackId = nextTrackId;
	clearReconnectTimer();
	connectionToken++;
	const token = connectionToken;

	if (socket) {
		socket.close();
		socket = null;
	}

	const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
	const params = new URLSearchParams({
		track_id: nextTrackId,
		channels: 'decoder',
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
				message: 'Decoder WebSocket connection failed',
				channel: 'decoder'
			};
		}
		ws.close();
	};

	ws.onmessage = (event) => {
		if (token !== connectionToken) {
			return;
		}

		let envelope: DecoderLiveEnvelope;
		try {
			envelope = JSON.parse(event.data) as DecoderLiveEnvelope;
		} catch {
			lastError = {
				code: 'invalid_json',
				message: 'Failed to parse decoder live message',
				channel: 'decoder'
			};
			return;
		}

		switch (envelope.kind) {
			case 'snapshot':
				snapshotRows = envelope.payload.rows;
				lastError = null;
				break;
			case 'event':
				handleP3Message(envelope.payload.message);
				lastError = null;
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
	connected = false;
	clearReconnectTimer();
	connectionToken++;
	if (socket) {
		socket.close();
		socket = null;
	}
}

export function getDecoderStore(): DecoderStore {
	return {
		get connected() {
			return connected;
		},
		get selectedTrackId() {
			return selectedTrackId;
		},
		get lastStatus() {
			return lastStatus;
		},
		get recentPassings() {
			return recentPassings;
		},
		get snapshotRows() {
			return snapshotRows;
		},
		get lastError() {
			return lastError;
		},
		get messageCount() {
			return messageCount;
		},
		connect,
		disconnect
	};
}
