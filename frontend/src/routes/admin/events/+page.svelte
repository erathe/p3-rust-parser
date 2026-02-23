<script lang="ts">
	import { events as eventsApi, tracks as tracksApi } from '$lib/api/client';
	import type { RaceEvent, Track } from '$lib/api/types';
	import { onMount } from 'svelte';

	let eventList = $state<RaceEvent[]>([]);
	let trackList = $state<Track[]>([]);
	let loading = $state(true);
	let error = $state('');

	// New event form
	let showForm = $state(false);
	let name = $state('');
	let date = $state(new Date().toISOString().split('T')[0]);
	let trackId = $state('');
	let saving = $state(false);

	onMount(async () => {
		await loadData();
	});

	async function loadData() {
		loading = true;
		try {
			[eventList, trackList] = await Promise.all([
				eventsApi.list(),
				tracksApi.list()
			]);
		} catch (e: any) {
			error = e.message;
		}
		loading = false;
	}

	async function createEvent(e: SubmitEvent) {
		e.preventDefault();
		if (!name || !trackId) return;
		saving = true;
		error = '';
		try {
			await eventsApi.create({ name, date, track_id: trackId });
			name = '';
			showForm = false;
			await loadData();
		} catch (err: any) {
			error = err.message;
		}
		saving = false;
	}

	async function deleteEvent(id: string) {
		if (!confirm('Delete this event and all its classes/motos?')) return;
		try {
			await eventsApi.delete(id);
			await loadData();
		} catch (err: any) {
			error = err.message;
		}
	}

	function statusColor(status: string): string {
		if (status === 'active') return 'text-green-400 bg-green-500/10';
		if (status === 'completed') return 'text-blue-400 bg-blue-500/10';
		return 'text-zinc-400 bg-zinc-800';
	}
</script>

<div class="space-y-6">
	<div class="flex items-center justify-between">
		<div>
			<h1 class="text-2xl font-bold">Events</h1>
			<p class="text-sm text-zinc-500">Race day event management</p>
		</div>
		<button
			onclick={() => (showForm = !showForm)}
			class="px-4 py-2 bg-amber-500 text-zinc-950 rounded-lg font-medium text-sm hover:bg-amber-400 transition-colors"
		>
			{showForm ? 'Cancel' : 'New Event'}
		</button>
	</div>

	{#if error}
		<div class="p-3 rounded-lg bg-red-500/10 text-red-400 text-sm">{error}</div>
	{/if}

	{#if showForm}
		<form onsubmit={createEvent} class="p-4 rounded-xl bg-zinc-900 border border-zinc-800 space-y-3">
			<h3 class="text-sm font-medium text-zinc-400">Create Event</h3>
			<div class="grid grid-cols-3 gap-3">
				<div>
					<label for="event_name" class="block text-xs text-zinc-500 mb-1">Event Name</label>
					<input
						id="event_name"
						type="text"
						bind:value={name}
						required
						placeholder="e.g. Saturday Race Day"
						class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm"
					/>
				</div>
				<div>
					<label for="event_date" class="block text-xs text-zinc-500 mb-1">Date</label>
					<input
						id="event_date"
						type="date"
						bind:value={date}
						required
						class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm"
					/>
				</div>
				<div>
					<label for="event_track" class="block text-xs text-zinc-500 mb-1">Track</label>
					<select
						id="event_track"
						bind:value={trackId}
						required
						class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm"
					>
						<option value="">Select track...</option>
						{#each trackList as track}
							<option value={track.id}>{track.name}</option>
						{/each}
					</select>
				</div>
			</div>
			<button
				type="submit"
				disabled={saving}
				class="px-4 py-2 bg-amber-500 text-zinc-950 rounded-lg font-medium text-sm hover:bg-amber-400 disabled:opacity-50"
			>
				{saving ? 'Creating...' : 'Create Event'}
			</button>
		</form>
	{/if}

	{#if loading}
		<div class="text-zinc-500">Loading...</div>
	{:else if eventList.length === 0}
		<div class="text-center py-12 text-zinc-500">
			<p class="text-lg">No events yet</p>
			<p class="text-sm mt-1">Create your first race event to get started</p>
		</div>
	{:else}
		<div class="space-y-3">
			{#each eventList as event}
				<a
					href="/admin/events/{event.id}"
					class="block p-4 rounded-xl bg-zinc-900 border border-zinc-800 hover:border-zinc-700 transition-colors"
				>
					<div class="flex items-center justify-between">
						<div>
							<h3 class="font-semibold text-zinc-200">{event.name}</h3>
							<p class="text-sm text-zinc-500 mt-0.5">{event.date}</p>
						</div>
						<div class="flex items-center gap-3">
							<span class="px-2 py-0.5 rounded text-xs font-medium {statusColor(event.status)}">
								{event.status.toUpperCase()}
							</span>
							<button
								onclick={(e) => { e.preventDefault(); e.stopPropagation(); deleteEvent(event.id); }}
								class="text-xs text-red-400 hover:bg-red-500/10 px-2 py-1 rounded"
							>
								Delete
							</button>
						</div>
					</div>
				</a>
			{/each}
		</div>
	{/if}
</div>
