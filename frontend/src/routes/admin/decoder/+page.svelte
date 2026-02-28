<script lang="ts">
	import { tracks as tracksApi } from '$lib/api/client';
	import type { Track } from '$lib/api/types';
	import { getDecoderStore } from '$lib/stores/decoder.svelte';
	import { onMount } from 'svelte';

	const decoder = getDecoderStore();
	let tracks = $state<Track[]>([]);
	let selectedTrackId = $state('');
	let loadingTracks = $state(true);
	let tracksError = $state<string | null>(null);
	let mounted = $state(false);

	function formatLastSeen(value: string | null): string {
		if (!value) return '-';
		const dt = new Date(value);
		if (Number.isNaN(dt.getTime())) return value;
		return dt.toLocaleString();
	}

	async function loadTracks() {
		loadingTracks = true;
		tracksError = null;
		try {
			tracks = await tracksApi.list();
			if (tracks.length > 0) {
				selectedTrackId = tracks[0].id;
			} else {
				selectedTrackId = '';
				decoder.disconnect();
			}
		} catch (error) {
			tracksError = error instanceof Error ? error.message : 'Failed to load tracks';
			tracks = [];
			selectedTrackId = '';
			decoder.disconnect();
		} finally {
			loadingTracks = false;
		}
	}

	$effect(() => {
		if (!mounted) {
			return;
		}

		if (selectedTrackId) {
			decoder.connect(selectedTrackId);
		} else {
			decoder.disconnect();
		}
	});

	onMount(() => {
		mounted = true;
		loadTracks();
		return () => decoder.disconnect();
	});
</script>

<div class="space-y-6">
	<div>
		<h1 class="text-2xl font-bold">Decoder Diagnostics</h1>
		<p class="text-sm text-zinc-500">Live connection status and decoder health</p>
	</div>

	<div class="p-4 rounded-xl bg-zinc-900 border border-zinc-800 space-y-3">
		<div class="text-sm font-medium text-zinc-300">Track</div>
		{#if loadingTracks}
			<p class="text-sm text-zinc-500">Loading tracks...</p>
		{:else if tracksError}
			<p class="text-sm text-red-400">{tracksError}</p>
		{:else if tracks.length === 0}
			<p class="text-sm text-zinc-500">
				No tracks found. Create a track first to stream decoder diagnostics.
			</p>
		{:else}
			<select
				bind:value={selectedTrackId}
				class="w-full sm:max-w-md rounded-lg border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm text-zinc-100 focus:outline-none focus:ring-2 focus:ring-zinc-600"
			>
				{#each tracks as track}
					<option value={track.id}>{track.name}</option>
				{/each}
			</select>
		{/if}
	</div>

	{#if !loadingTracks && tracks.length === 0}
		<div class="p-4 rounded-xl bg-zinc-900 border border-zinc-800 text-sm text-zinc-500">
			Decoder diagnostics will appear here once a track is created and selected.
		</div>
	{:else}
		<!-- Connection Status -->
		<div class="p-4 rounded-xl bg-zinc-900 border border-zinc-800">
			<div class="flex items-center gap-3">
				<div
					class="w-3 h-3 rounded-full {decoder.connected
						? 'bg-green-500 shadow-[0_0_8px_rgba(34,197,94,0.5)]'
						: 'bg-red-500 shadow-[0_0_8px_rgba(239,68,68,0.5)]'}"
				></div>
				<span class="font-medium">
					{decoder.connected ? 'Connected' : 'Disconnected'}
				</span>
				<span class="text-sm text-zinc-500 ml-auto">
					{decoder.messageCount} messages received
				</span>
			</div>
			{#if decoder.lastError}
				<div class="mt-3 rounded-lg border border-red-500/20 bg-red-950/40 px-3 py-2 text-sm text-red-300">
					{decoder.lastError.message}
					{#if decoder.lastError.code}
						<span class="ml-2 font-mono text-red-200/80">({decoder.lastError.code})</span>
					{/if}
				</div>
			{/if}
		</div>

		<!-- Decoder Status -->
		{#if decoder.lastStatus}
			<div class="grid grid-cols-2 sm:grid-cols-4 gap-4">
				<div class="p-4 rounded-xl bg-zinc-900 border border-zinc-800">
					<div class="text-xs text-zinc-500 uppercase tracking-wider">Noise</div>
					<div class="text-2xl font-bold mt-1 font-mono">{decoder.lastStatus.noise}</div>
				</div>
				<div class="p-4 rounded-xl bg-zinc-900 border border-zinc-800">
					<div class="text-xs text-zinc-500 uppercase tracking-wider">Temperature</div>
					<div class="text-2xl font-bold mt-1 font-mono">
						{(decoder.lastStatus.temperature / 10).toFixed(1)}&deg;C
					</div>
				</div>
				<div class="p-4 rounded-xl bg-zinc-900 border border-zinc-800">
					<div class="text-xs text-zinc-500 uppercase tracking-wider">GPS</div>
					<div class="text-2xl font-bold mt-1">
						{decoder.lastStatus.gps_status === 1 ? 'Locked' : 'No Fix'}
					</div>
				</div>
				<div class="p-4 rounded-xl bg-zinc-900 border border-zinc-800">
					<div class="text-xs text-zinc-500 uppercase tracking-wider">Satellites</div>
					<div class="text-2xl font-bold mt-1 font-mono">{decoder.lastStatus.satellites}</div>
				</div>
			</div>
			{#if decoder.lastStatus.decoder_id}
				<div class="text-sm text-zinc-500">
					Latest decoder: <span class="font-mono text-zinc-400">{decoder.lastStatus.decoder_id}</span>
				</div>
			{/if}
		{:else}
			<div class="text-zinc-500 text-sm">Waiting for decoder status messages...</div>
		{/if}

		<!-- Decoder Snapshot -->
		<div>
			<h2 class="text-lg font-semibold text-zinc-300 mb-3">Decoder Snapshot</h2>
			{#if decoder.snapshotRows.length === 0}
				<p class="text-sm text-zinc-500">No decoder snapshot rows available for this track yet</p>
			{:else}
				<div class="overflow-hidden rounded-xl border border-zinc-800">
					<table class="w-full text-sm">
						<thead class="bg-zinc-900">
							<tr class="text-left text-zinc-500">
								<th class="px-4 py-2 font-medium">Loop</th>
								<th class="px-4 py-2 font-medium">Decoder</th>
								<th class="px-4 py-2 font-medium">Noise</th>
								<th class="px-4 py-2 font-medium">Temp</th>
								<th class="px-4 py-2 font-medium">GPS</th>
								<th class="px-4 py-2 font-medium">Sats</th>
								<th class="px-4 py-2 font-medium">Last Seen</th>
							</tr>
						</thead>
						<tbody class="divide-y divide-zinc-800/50">
							{#each decoder.snapshotRows as row}
								<tr class="hover:bg-zinc-900/50">
									<td class="px-4 py-2 text-zinc-200">
										{row.loop_name}
										<span class="ml-2 text-xs text-zinc-500">#{row.loop_position}</span>
									</td>
									<td class="px-4 py-2 font-mono text-zinc-400">{row.decoder_id}</td>
									<td class="px-4 py-2 font-mono text-zinc-400">{row.noise ?? '-'}</td>
									<td class="px-4 py-2 font-mono text-zinc-400">
										{row.temperature === null ? '-' : `${(row.temperature / 10).toFixed(1)} C`}
									</td>
									<td class="px-4 py-2 text-zinc-400">
										{row.gps_status === null ? '-' : row.gps_status === 1 ? 'Locked' : 'No Fix'}
									</td>
									<td class="px-4 py-2 font-mono text-zinc-400">{row.satellites ?? '-'}</td>
									<td class="px-4 py-2 text-zinc-500">{formatLastSeen(row.last_seen)}</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{/if}
		</div>

		<!-- Recent Passings -->
		<div>
			<h2 class="text-lg font-semibold text-zinc-300 mb-3">Recent Passings</h2>
			{#if decoder.recentPassings.length === 0}
				<p class="text-sm text-zinc-500">No passing events received yet</p>
			{:else}
				<div class="overflow-hidden rounded-xl border border-zinc-800">
					<table class="w-full text-sm">
						<thead class="bg-zinc-900">
							<tr class="text-left text-zinc-500">
								<th class="px-4 py-2 font-medium">#</th>
								<th class="px-4 py-2 font-medium">Transponder</th>
								<th class="px-4 py-2 font-medium">String</th>
								<th class="px-4 py-2 font-medium">Strength</th>
								<th class="px-4 py-2 font-medium">Hits</th>
								<th class="px-4 py-2 font-medium">Decoder</th>
							</tr>
						</thead>
						<tbody class="divide-y divide-zinc-800/50">
							{#each decoder.recentPassings.slice(0, 20) as passing}
								<tr class="hover:bg-zinc-900/50">
									<td class="px-4 py-2 font-mono text-zinc-500">{passing.passing_number}</td>
									<td class="px-4 py-2 font-mono {passing.transponder_id >= 9991 &&
									passing.transponder_id <= 9995
										? 'text-amber-400 font-bold'
										: 'text-zinc-200'}">
										{passing.transponder_id}
										{#if passing.transponder_id >= 9991 && passing.transponder_id <= 9995}
											<span class="text-xs text-amber-500/70 ml-1">GATE</span>
										{/if}
									</td>
									<td class="px-4 py-2 font-mono text-zinc-400">
										{passing.transponder_string || '-'}
									</td>
									<td class="px-4 py-2 font-mono text-zinc-400">
										{passing.strength ?? '-'}
									</td>
									<td class="px-4 py-2 font-mono text-zinc-400">{passing.hits ?? '-'}</td>
									<td class="px-4 py-2 font-mono text-zinc-500">
										{passing.decoder_id || '-'}
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{/if}
		</div>
	{/if}
</div>
