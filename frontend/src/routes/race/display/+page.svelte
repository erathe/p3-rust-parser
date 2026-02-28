<script lang="ts">
	import { getRaceStore } from '$lib/stores/race.svelte';
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import TimingTower from '$lib/components/race/TimingTower.svelte';

	const race = getRaceStore();
	const trackId = $derived(page.url.searchParams.get('track_id')?.trim() ?? '');
	let mounted = $state(false);
	let trackInput = $state('');

	$effect(() => {
		if (trackId && trackInput !== trackId) {
			trackInput = trackId;
		}
	});

	$effect(() => {
		if (!mounted) {
			return;
		}

		if (trackId) {
			race.connect(trackId);
		} else {
			race.disconnect();
		}
	});

	onMount(() => {
		mounted = true;
		return () => race.disconnect();
	});

	async function openTrackStream() {
		const nextTrackId = trackInput.trim();
		if (!nextTrackId) {
			return;
		}

		const params = new URLSearchParams(page.url.searchParams);
		params.set('track_id', nextTrackId);
		await goto(`${page.url.pathname}?${params.toString()}`, {
			replaceState: true,
			noScroll: true,
			keepFocus: true
		});
	}
</script>

<svelte:head>
	<title>Race Display | P3 BMX Timing</title>
</svelte:head>

<div class="fixed inset-0 bg-zinc-950 p-4">
	{#if !trackId}
		<div class="h-full flex items-center justify-center">
			<div class="w-full max-w-md rounded-xl border border-zinc-800 bg-zinc-900 p-4">
				<div class="text-sm text-zinc-400">
					This page needs a <span class="mx-1 font-mono text-zinc-200">track_id</span> query parameter.
				</div>
				<form
					class="mt-3 flex gap-2"
					onsubmit={(event) => {
						event.preventDefault();
						void openTrackStream();
					}}
				>
					<input
						bind:value={trackInput}
						placeholder="track-a"
						class="flex-1 rounded-lg border border-zinc-700 bg-zinc-950 px-3 py-2 text-sm text-zinc-100"
					/>
					<button
						type="submit"
						disabled={!trackInput.trim()}
						class="rounded-lg bg-amber-500 px-3 py-2 text-sm font-medium text-zinc-950 disabled:cursor-not-allowed disabled:bg-zinc-700 disabled:text-zinc-500"
					>
						Open
					</button>
				</form>
			</div>
		</div>
	{:else}
		<TimingTower
			phase={race.phase}
			className={race.className}
			roundType={race.roundType}
			positions={race.positions}
			results={race.results}
			finishedCount={race.finishedCount}
			totalRiders={race.totalRiders}
		/>
	{/if}
</div>
