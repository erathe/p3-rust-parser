<script lang="ts">
	import { getRaceStore } from '$lib/stores/race.svelte';
	import { onMount } from 'svelte';
	import { race as raceApi, events as eventsApi, motos as motosApi } from '$lib/api/client';
	import type { RaceEvent, Moto, MotoWithEntries } from '$lib/api/types';
	import TimingTower from '$lib/components/race/TimingTower.svelte';

	const race = getRaceStore();

	let eventList = $state<RaceEvent[]>([]);
	let selectedEventId = $state<string>('');
	let eventMotos = $state<Moto[]>([]);
	let selectedMotoId = $state<string>('');
	let selectedMotoDetail = $state<MotoWithEntries | null>(null);
	let trackId = $state<string>('');
	let error = $state<string>('');
	let loading = $state(false);

	onMount(() => {
		race.connect();
		eventsApi.list().then((e) => { eventList = e; }).catch(() => {});
		return () => race.disconnect();
	});

	async function loadEventMotos() {
		if (!selectedEventId) return;
		try {
			const event = await eventsApi.get(selectedEventId);
			trackId = event.track_id;
			eventMotos = await eventsApi.listMotos(selectedEventId);
		} catch (e: any) {
			error = e.message;
		}
	}

	async function loadMotoDetail() {
		if (!selectedMotoId) {
			selectedMotoDetail = null;
			return;
		}
		try {
			selectedMotoDetail = await motosApi.get(selectedMotoId);
		} catch (e: any) {
			error = e.message;
		}
	}

	async function handleStage() {
		if (!selectedMotoId || !trackId) return;
		loading = true;
		error = '';
		try {
			await raceApi.stage(selectedMotoId, trackId);
		} catch (e: any) {
			error = e.message;
		}
		loading = false;
	}

	async function handleReset() {
		loading = true;
		error = '';
		try {
			await raceApi.reset();
		} catch (e: any) {
			error = e.message;
		}
		loading = false;
	}

	async function handleForceFinish() {
		loading = true;
		error = '';
		try {
			await raceApi.forceFinish();
		} catch (e: any) {
			error = e.message;
		}
		loading = false;
	}

	function roundTypeLabel(rt: string): string {
		const labels: Record<string, string> = {
			moto1: 'Moto 1', moto2: 'Moto 2', moto3: 'Moto 3',
			quarter: 'Quarter', semi: 'Semi', main: 'Main'
		};
		return labels[rt] ?? rt;
	}
</script>

<svelte:head>
	<title>Race Control | P3 BMX Timing</title>
</svelte:head>

<div class="grid grid-cols-1 lg:grid-cols-[400px_1fr] gap-6 h-[calc(100vh-5rem)]">
	<!-- Control Panel -->
	<div class="space-y-4 overflow-y-auto">
		<div>
			<h1 class="text-2xl font-bold">Race Control</h1>
			<p class="text-sm text-zinc-500">Manage race staging and progression</p>
		</div>

		<!-- Connection Status -->
		<div class="p-3 rounded-xl bg-zinc-900 border border-zinc-800 flex items-center gap-3">
			<div class="w-2.5 h-2.5 rounded-full {race.connected ? 'bg-green-500' : 'bg-red-500'}"></div>
			<span class="text-sm">{race.connected ? 'Connected' : 'Disconnected'}</span>
			<span class="text-sm text-zinc-500 ml-auto font-mono">{race.phase.toUpperCase()}</span>
		</div>

		{#if error}
			<div class="p-3 rounded-xl bg-red-500/10 border border-red-500/20 text-red-400 text-sm">
				{error}
			</div>
		{/if}

		<!-- Event & Moto Selection -->
		<div class="p-4 rounded-xl bg-zinc-900 border border-zinc-800 space-y-3">
			<h3 class="text-sm font-medium text-zinc-400 uppercase tracking-wider">Load Moto</h3>

			<div>
				<label for="event-select" class="block text-xs text-zinc-500 mb-1">Event</label>
				<select
					id="event-select"
					bind:value={selectedEventId}
					onchange={loadEventMotos}
					class="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm"
				>
					<option value="">Select event...</option>
					{#each eventList as event}
						<option value={event.id}>{event.name} — {event.date}</option>
					{/each}
				</select>
			</div>

			{#if eventMotos.length > 0}
				<div>
					<label for="moto-select" class="block text-xs text-zinc-500 mb-1">Moto</label>
					<select
						id="moto-select"
						bind:value={selectedMotoId}
						onchange={loadMotoDetail}
						class="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm"
					>
						<option value="">Select moto...</option>
						{#each eventMotos as moto}
							<option value={moto.id}>
								{roundTypeLabel(moto.round_type)}
								{moto.round_number ? `#${moto.round_number}` : ''}
								— {moto.status}
							</option>
						{/each}
					</select>
				</div>
			{/if}

			{#if selectedMotoDetail}
				<div class="text-sm text-zinc-400 space-y-1">
					<p class="font-medium text-zinc-300">
						{roundTypeLabel(selectedMotoDetail.round_type)}
						{selectedMotoDetail.round_number ? `#${selectedMotoDetail.round_number}` : ''}
					</p>
					<p>{selectedMotoDetail.entries.length} riders:</p>
					<div class="grid grid-cols-2 gap-1 text-xs">
						{#each selectedMotoDetail.entries as entry}
							<div class="flex items-center gap-2">
								<span class="text-amber-400 font-bold">L{entry.lane}</span>
								{#if entry.rider}
									<span>{entry.rider.plate_number} — {entry.rider.first_name} {entry.rider.last_name}</span>
								{:else}
									<span class="text-zinc-600">Unknown rider</span>
								{/if}
							</div>
						{/each}
					</div>
				</div>
			{/if}

			<button
				onclick={handleStage}
				disabled={!selectedMotoId || loading || race.phase === 'racing'}
				class="w-full py-2.5 rounded-lg font-bold text-sm transition-colors
					{!selectedMotoId || loading || race.phase === 'racing'
						? 'bg-zinc-800 text-zinc-600 cursor-not-allowed'
						: 'bg-amber-500 text-zinc-950 hover:bg-amber-400'}"
			>
				{loading ? 'Loading...' : 'Stage Moto'}
			</button>
		</div>

		<!-- Race Actions -->
		<div class="p-4 rounded-xl bg-zinc-900 border border-zinc-800 space-y-3">
			<h3 class="text-sm font-medium text-zinc-400 uppercase tracking-wider">Race Actions</h3>

			<div class="grid grid-cols-2 gap-2">
				<button
					onclick={handleForceFinish}
					disabled={race.phase !== 'racing' || loading}
					class="py-2 rounded-lg font-medium text-sm transition-colors
						{race.phase !== 'racing' || loading
							? 'bg-zinc-800 text-zinc-600 cursor-not-allowed'
							: 'bg-red-500/20 text-red-400 hover:bg-red-500/30 border border-red-500/30'}"
				>
					Force Finish
				</button>
				<button
					onclick={handleReset}
					disabled={race.phase === 'idle' || loading}
					class="py-2 rounded-lg font-medium text-sm transition-colors
						{race.phase === 'idle' || loading
							? 'bg-zinc-800 text-zinc-600 cursor-not-allowed'
							: 'bg-zinc-700 text-zinc-300 hover:bg-zinc-600'}"
				>
					Reset
				</button>
			</div>
		</div>

		<!-- Race Info -->
		{#if race.phase !== 'idle'}
			<div class="p-4 rounded-xl bg-zinc-900 border border-zinc-800 space-y-2">
				<h3 class="text-sm font-medium text-zinc-400 uppercase tracking-wider">Current Race</h3>
				<div class="text-sm space-y-1">
					<p><span class="text-zinc-500">Class:</span> <span class="text-white">{race.className ?? '-'}</span></p>
					<p><span class="text-zinc-500">Round:</span> <span class="text-white">{race.roundType ?? '-'}</span></p>
					<p><span class="text-zinc-500">Riders:</span> <span class="text-white">{race.totalRiders}</span></p>
					<p><span class="text-zinc-500">Finished:</span> <span class="text-white">{race.finishedCount}/{race.totalRiders}</span></p>
				</div>
			</div>
		{/if}
	</div>

	<!-- Live Tower Preview -->
	<div class="hidden lg:block">
		<TimingTower
			phase={race.phase}
			className={race.className}
			roundType={race.roundType}
			positions={race.positions}
			results={race.results}
			finishedCount={race.finishedCount}
			totalRiders={race.totalRiders}
		/>
	</div>
</div>
