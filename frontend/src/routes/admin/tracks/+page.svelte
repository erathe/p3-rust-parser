<script lang="ts">
	import { tracks } from '$lib/api/client';
	import type { Track } from '$lib/api/types';

	let trackList = $state<Track[]>([]);
	let loading = $state(true);

	$effect(() => {
		tracks.list().then((t) => {
			trackList = t;
			loading = false;
		});
	});

	async function handleDelete(id: string) {
		if (!confirm('Delete this track and all its timing loops?')) return;
		await tracks.delete(id);
		trackList = trackList.filter((t) => t.id !== id);
	}
</script>

<div class="space-y-6">
	<div class="flex items-center justify-between">
		<div>
			<h1 class="text-2xl font-bold">Tracks</h1>
			<p class="text-sm text-zinc-500">Configure your BMX tracks and timing loops</p>
		</div>
		<a
			href="/admin/tracks/new"
			class="px-4 py-2 bg-amber-500 text-black font-medium rounded-lg hover:bg-amber-400 transition-colors text-sm"
		>
			New Track
		</a>
	</div>

	{#if loading}
		<div class="text-zinc-500">Loading...</div>
	{:else if trackList.length === 0}
		<div class="text-center py-12 text-zinc-500">
			<p class="text-lg">No tracks configured yet</p>
			<p class="text-sm mt-1">Create your first track to get started</p>
		</div>
	{:else}
		<div class="grid gap-3">
			{#each trackList as track}
				<a
					href="/admin/tracks/{track.id}"
					class="flex items-center justify-between p-4 rounded-xl bg-zinc-900 border border-zinc-800 hover:border-zinc-700 transition-colors group"
				>
					<div>
						<div class="font-medium text-zinc-100 group-hover:text-white">
							{track.name}
						</div>
						<div class="text-sm text-zinc-500 mt-0.5">
							{track.hill_type} hill &middot; Gate beacon: {track.gate_beacon_id}
						</div>
					</div>
					<button
						class="px-3 py-1 text-xs text-red-400 hover:bg-red-500/10 rounded-md transition-colors opacity-0 group-hover:opacity-100"
						onclick={(e) => {
							e.preventDefault();
							handleDelete(track.id);
						}}
					>
						Delete
					</button>
				</a>
			{/each}
		</div>
	{/if}
</div>
