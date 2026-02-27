import type {
	Track,
	TrackWithLoops,
	TimingLoop,
	TrackSection,
	SectionInput,
	Rider,
	CreateTrackRequest,
	CreateLoopRequest,
	CreateRiderRequest,
	RaceEvent,
	EventWithClasses,
	EventClass,
	Moto,
	MotoWithEntries,
	RaceStateResponse,
	TrackOnboardingDiscoveryResponse
} from './types';

const BASE = '/api';

async function request<T>(path: string, options?: RequestInit): Promise<T> {
	const res = await fetch(`${BASE}${path}`, {
		headers: { 'Content-Type': 'application/json' },
		...options
	});
	if (!res.ok) {
		const err = await res.json().catch(() => ({ error: res.statusText }));
		throw new Error(err.error || res.statusText);
	}
	if (res.status === 204) return undefined as T;
	return res.json();
}

// Tracks
export const tracks = {
	list: () => request<Track[]>('/tracks'),
	get: (id: string) => request<TrackWithLoops>(`/tracks/${id}`),
	create: (data: CreateTrackRequest) =>
		request<Track>('/tracks', { method: 'POST', body: JSON.stringify(data) }),
	update: (id: string, data: CreateTrackRequest) =>
		request<Track>(`/tracks/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
	delete: (id: string) => request<void>(`/tracks/${id}`, { method: 'DELETE' }),

	createLoop: (trackId: string, data: CreateLoopRequest) =>
		request<TimingLoop>(`/tracks/${trackId}/loops`, {
			method: 'POST',
			body: JSON.stringify(data)
		}),
	updateLoop: (trackId: string, loopId: string, data: CreateLoopRequest) =>
		request<TimingLoop>(`/tracks/${trackId}/loops/${loopId}`, {
			method: 'PUT',
			body: JSON.stringify(data)
		}),
	deleteLoop: (trackId: string, loopId: string) =>
		request<void>(`/tracks/${trackId}/loops/${loopId}`, { method: 'DELETE' }),

	saveSections: (trackId: string, sections: SectionInput[]) =>
		request<TrackSection[]>(`/tracks/${trackId}/sections`, {
			method: 'PUT',
			body: JSON.stringify({ sections })
		})
};

// Riders
export const riders = {
	list: (search?: string) => {
		const params = search ? `?search=${encodeURIComponent(search)}` : '';
		return request<Rider[]>(`/riders${params}`);
	},
	get: (id: string) => request<Rider>(`/riders/${id}`),
	create: (data: CreateRiderRequest) =>
		request<Rider>('/riders', { method: 'POST', body: JSON.stringify(data) }),
	update: (id: string, data: CreateRiderRequest) =>
		request<Rider>(`/riders/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
	delete: (id: string) => request<void>(`/riders/${id}`, { method: 'DELETE' })
};

// Events
export const events = {
	list: () => request<RaceEvent[]>('/events'),
	get: (id: string) => request<EventWithClasses>(`/events/${id}`),
	create: (data: { name: string; date: string; track_id: string }) =>
		request<RaceEvent>('/events', { method: 'POST', body: JSON.stringify(data) }),
	update: (id: string, data: { name: string; date: string; status: string }) =>
		request<RaceEvent>(`/events/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
	delete: (id: string) => request<void>(`/events/${id}`, { method: 'DELETE' }),

	createClass: (eventId: string, data: { name: string; race_format?: string; scoring?: string }) =>
		request<EventClass>(`/events/${eventId}/classes`, {
			method: 'POST',
			body: JSON.stringify(data)
		}),
	deleteClass: (eventId: string, classId: string) =>
		request<void>(`/events/${eventId}/classes/${classId}`, { method: 'DELETE' }),

	addClassRider: (eventId: string, classId: string, riderId: string) =>
		request<void>(`/events/${eventId}/classes/${classId}/riders`, {
			method: 'POST',
			body: JSON.stringify({ rider_id: riderId })
		}),
	removeClassRider: (eventId: string, classId: string, riderId: string) =>
		request<void>(`/events/${eventId}/classes/${classId}/riders/${riderId}`, {
			method: 'DELETE'
		}),

	listMotos: (eventId: string) =>
		request<Moto[]>(`/events/${eventId}/motos`),
	listClassMotos: (eventId: string, classId: string) =>
		request<Moto[]>(`/events/${eventId}/classes/${classId}/motos`),
	generateMotos: (eventId: string, classId: string) =>
		request<{ format: string; motos_created: number }>(
			`/events/${eventId}/classes/${classId}/generate-motos`,
			{ method: 'POST' }
		)
};

// Motos
export const motos = {
	get: (id: string) => request<MotoWithEntries>(`/motos/${id}`)
};

// Seed demo data
export const seed = {
	demo: () =>
		request<{ track_id: string; event_id: string; class_id: string; riders_created: number; motos_created: number }>(
			'/seed-demo',
			{ method: 'POST' }
		)
};

// Race control
export const race = {
	getState: () => request<RaceStateResponse>('/race/state'),
	stage: (motoId: string, trackId: string) =>
		request<RaceStateResponse>('/race/stage', {
			method: 'POST',
			body: JSON.stringify({ moto_id: motoId, track_id: trackId })
		}),
	reset: () => request<RaceStateResponse>('/race/reset', { method: 'POST' }),
	forceFinish: () => request<RaceStateResponse>('/race/force-finish', { method: 'POST' })
};

// Track onboarding
export const onboarding = {
	discovery: (trackId: string, params?: { window_seconds?: number; max_messages?: number }) => {
		const search = new URLSearchParams();
		if (params?.window_seconds !== undefined) {
			search.set('window_seconds', String(params.window_seconds));
		}
		if (params?.max_messages !== undefined) {
			search.set('max_messages', String(params.max_messages));
		}
		const suffix = search.toString() ? `?${search.toString()}` : '';
		return request<TrackOnboardingDiscoveryResponse>(
			`/tracks/${trackId}/onboarding/discovery${suffix}`
		);
	}
};
