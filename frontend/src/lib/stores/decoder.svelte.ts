import type { P3Message, StatusMessage, PassingMessage } from '$lib/api/types';

let connected = $state(false);
let socket = $state<WebSocket | null>(null);
let lastStatus = $state<StatusMessage | null>(null);
let recentPassings = $state<PassingMessage[]>([]);
let messageCount = $state(0);

const MAX_RECENT = 50;

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
		// Reconnect after 3 seconds
		setTimeout(connect, 3000);
	};

	ws.onerror = () => {
		ws.close();
	};

	ws.onmessage = (event) => {
		const msg: P3Message = JSON.parse(event.data);
		messageCount++;

		if (msg.message_type === 'STATUS') {
			lastStatus = msg;
		} else if (msg.message_type === 'PASSING') {
			recentPassings = [msg, ...recentPassings.slice(0, MAX_RECENT - 1)];
		}
	};
}

export function getDecoderStore() {
	return {
		get connected() {
			return connected;
		},
		get lastStatus() {
			return lastStatus;
		},
		get recentPassings() {
			return recentPassings;
		},
		get messageCount() {
			return messageCount;
		},
		connect,
		disconnect() {
			socket?.close();
		}
	};
}
