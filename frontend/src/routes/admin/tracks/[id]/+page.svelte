<script lang="ts">
	import { page } from '$app/stores';
	import { tracks } from '$lib/api/client';
	import type { TrackWithLoops, TimingLoop } from '$lib/api/types';
	import TrackBuilder from '$lib/components/admin/track/TrackBuilder.svelte';

	let track = $state<TrackWithLoops | null>(null);
	let loading = $state(true);
	let error = $state('');

	// New loop form
	let newLoopName = $state('');
	let newLoopDecoderId = $state('');
	let newLoopIsStart = $state(false);
	let newLoopIsFinish = $state(false);
	let addingLoop = $state(false);

	const trackId = $derived($page.params.id!);

	$effect(() => {
		loadTrack();
	});

	async function loadTrack() {
		loading = true;
		try {
			track = await tracks.get(trackId);
		} catch (err) {
			error = err instanceof Error ? err.message : 'Failed to load track';
		} finally {
			loading = false;
		}
	}

	async function addLoop(e: SubmitEvent) {
		e.preventDefault();
		if (!track) return;
		addingLoop = true;
		error = '';

		try {
			const nextPosition = track.loops.length;
			await tracks.createLoop(trackId, {
				name: newLoopName,
				decoder_id: newLoopDecoderId,
				position: nextPosition,
				is_start: newLoopIsStart,
				is_finish: newLoopIsFinish
			});
			newLoopName = '';
			newLoopDecoderId = '';
			newLoopIsStart = false;
			newLoopIsFinish = false;
			await loadTrack();
		} catch (err) {
			error = err instanceof Error ? err.message : 'Failed to add loop';
		} finally {
			addingLoop = false;
		}
	}

	async function deleteLoop(loopId: string) {
		if (!confirm('Delete this timing loop?')) return;
		await tracks.deleteLoop(trackId, loopId);
		await loadTrack();
	}

	function loopTypeLabel(loop: TimingLoop): string {
		if (loop.is_start) return 'START';
		if (loop.is_finish) return 'FINISH';
		return 'SPLIT';
	}

	function loopTypeColor(loop: TimingLoop): string {
		if (loop.is_start) return 'text-green-400 bg-green-500/10';
		if (loop.is_finish) return 'text-red-400 bg-red-500/10';
		return 'text-blue-400 bg-blue-500/10';
	}
</script>

<div class="space-y-6">
	<a href="/admin/tracks" class="text-sm text-zinc-500 hover:text-zinc-300">&larr; Back to tracks</a>

	{#if loading}
		<div class="text-zinc-500">Loading...</div>
	{:else if !track}
		<div class="text-red-400">Track not found</div>
	{:else}
		<div>
			<h1 class="text-2xl font-bold">{track.name}</h1>
			<p class="text-sm text-zinc-500 mt-0.5">
				{track.hill_type} hill &middot; Gate beacon: {track.gate_beacon_id}
			</p>
		</div>

		{#if error}
			<div class="p-3 rounded-lg bg-red-500/10 text-red-400 text-sm">{error}</div>
		{/if}

		<!-- Track Layout Builder -->
		{#key track.id + '-' + track.sections.length}
			<TrackBuilder {track} onUpdate={loadTrack} />
		{/key}

		<!-- Timing Loops -->
		<div class="space-y-3">
			<h2 class="text-lg font-semibold text-zinc-300">Timing Loops (Decoders)</h2>

			{#if track.loops.length === 0}
				<p class="text-sm text-zinc-500">
					No timing loops configured. Add loops in the order a rider passes them.
				</p>
			{:else}
				<div class="space-y-2">
					{#each track.loops as loop, i}
						<div
							class="flex items-center gap-3 p-3 rounded-lg bg-zinc-900 border border-zinc-800"
						>
							<div
								class="w-8 h-8 rounded-full bg-zinc-800 flex items-center justify-center text-sm font-mono text-zinc-400"
							>
								{i}
							</div>
							<div class="flex-1">
								<div class="font-medium text-zinc-200">{loop.name}</div>
								<div class="text-xs text-zinc-500 font-mono">{loop.decoder_id}</div>
							</div>
							<span
								class="px-2 py-0.5 rounded text-xs font-medium {loopTypeColor(loop)}"
							>
								{loopTypeLabel(loop)}
							</span>
							<button
								class="text-xs text-red-400 hover:bg-red-500/10 px-2 py-1 rounded"
								onclick={() => deleteLoop(loop.id)}
							>
								Remove
							</button>
						</div>
					{/each}
				</div>
			{/if}

			<!-- Add loop form -->
			<form
				onsubmit={addLoop}
				class="p-4 rounded-lg bg-zinc-900/50 border border-zinc-800 border-dashed space-y-3"
			>
				<h3 class="text-sm font-medium text-zinc-400">Add Timing Loop</h3>
				<div class="grid grid-cols-2 gap-3">
					<div>
						<label for="loop_name" class="block text-xs text-zinc-500 mb-1">Name</label>
						<input
							id="loop_name"
							type="text"
							bind:value={newLoopName}
							required
							placeholder="e.g. Start Hill"
							class="w-full px-3 py-1.5 bg-zinc-900 border border-zinc-700 rounded-lg text-sm text-zinc-100 placeholder:text-zinc-600 focus:outline-none focus:border-amber-500/50"
						/>
					</div>
					<div>
						<label for="decoder_id" class="block text-xs text-zinc-500 mb-1">Decoder ID</label>
						<input
							id="decoder_id"
							type="text"
							bind:value={newLoopDecoderId}
							required
							placeholder="e.g. D0000C00"
							class="w-full px-3 py-1.5 bg-zinc-900 border border-zinc-700 rounded-lg text-sm text-zinc-100 placeholder:text-zinc-600 font-mono focus:outline-none focus:border-amber-500/50"
						/>
					</div>
				</div>
				<div class="flex gap-4 items-center">
					<label class="flex items-center gap-2 text-sm text-zinc-400">
						<input type="checkbox" bind:checked={newLoopIsStart} class="accent-green-500" />
						Start loop
					</label>
					<label class="flex items-center gap-2 text-sm text-zinc-400">
						<input type="checkbox" bind:checked={newLoopIsFinish} class="accent-red-500" />
						Finish loop
					</label>
					<button
						type="submit"
						disabled={addingLoop || !newLoopName || !newLoopDecoderId}
						class="ml-auto px-3 py-1.5 bg-zinc-700 text-zinc-200 rounded-lg text-sm hover:bg-zinc-600 transition-colors disabled:opacity-50"
					>
						{addingLoop ? 'Adding...' : 'Add Loop'}
					</button>
				</div>
			</form>
		</div>
	{/if}
</div>
