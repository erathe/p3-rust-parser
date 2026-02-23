<script lang="ts">
	import { tracks, riders, seed, events as eventsApi } from '$lib/api/client';
	import type { Track, RaceEvent } from '$lib/api/types';

	let trackList = $state<Track[]>([]);
	let riderCount = $state(0);
	let eventList = $state<RaceEvent[]>([]);
	let seeding = $state(false);
	let seedResult = $state('');

	$effect(() => {
		tracks.list().then((t) => (trackList = t));
		riders.list().then((r) => (riderCount = r.length));
		eventsApi.list().then((e) => (eventList = e));
	});

	async function seedDemo() {
		seeding = true;
		seedResult = '';
		try {
			const result = await seed.demo();
			if (result.riders_created > 0) {
				seedResult = `Created ${result.riders_created} riders, ${result.motos_created} motos`;
			} else {
				seedResult = 'Demo data already exists';
			}
			// Refresh counts
			tracks.list().then((t) => (trackList = t));
			riders.list().then((r) => (riderCount = r.length));
			eventsApi.list().then((e) => (eventList = e));
		} catch (e: any) {
			seedResult = `Error: ${e.message}`;
		}
		seeding = false;
	}
</script>

<div class="space-y-8">
	<div>
		<h1 class="text-3xl font-bold tracking-tight">
			<span class="text-amber-400">P3</span> BMX Timing
		</h1>
		<p class="text-zinc-500 mt-1">Local-first race timing and display system</p>
	</div>

	<div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
		<a
			href="/admin/tracks"
			class="block p-6 rounded-xl bg-zinc-900 border border-zinc-800 hover:border-zinc-700 transition-colors"
		>
			<div class="text-3xl font-bold text-amber-400">{trackList.length}</div>
			<div class="text-sm text-zinc-400 mt-1">Tracks configured</div>
		</a>

		<a
			href="/admin/riders"
			class="block p-6 rounded-xl bg-zinc-900 border border-zinc-800 hover:border-zinc-700 transition-colors"
		>
			<div class="text-3xl font-bold text-amber-400">{riderCount}</div>
			<div class="text-sm text-zinc-400 mt-1">Riders registered</div>
		</a>

		<a
			href="/admin/events"
			class="block p-6 rounded-xl bg-zinc-900 border border-zinc-800 hover:border-zinc-700 transition-colors"
		>
			<div class="text-3xl font-bold text-amber-400">{eventList.length}</div>
			<div class="text-sm text-zinc-400 mt-1">Events</div>
		</a>
	</div>

	<!-- Quick Start -->
	<div class="p-6 rounded-xl bg-zinc-900 border border-zinc-800 space-y-4">
		<h2 class="text-lg font-semibold">Quick Start</h2>
		<p class="text-sm text-zinc-400">
			Seed demo data matching the test server's <code class="text-amber-400/80">full-race</code> scenario
			(8 riders, track with 3 timing loops, event with motos). Then go to Race Control to stage and run races.
		</p>

		<div class="flex items-center gap-4">
			<button
				onclick={seedDemo}
				disabled={seeding}
				class="px-4 py-2 rounded-lg font-medium text-sm transition-colors bg-amber-500 text-zinc-950 hover:bg-amber-400 disabled:opacity-50"
			>
				{seeding ? 'Seeding...' : 'Seed Demo Data'}
			</button>

			{#if seedResult}
				<span class="text-sm text-zinc-400">{seedResult}</span>
			{/if}
		</div>
	</div>

	<!-- Race Pages -->
	<div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
		<a
			href="/race/control"
			class="block p-6 rounded-xl bg-zinc-900 border border-amber-500/30 hover:border-amber-500/50 transition-colors"
		>
			<div class="text-lg font-bold text-amber-400">Race Control</div>
			<div class="text-sm text-zinc-400 mt-1">Stage motos, manage races</div>
		</a>

		<a
			href="/race/display"
			class="block p-6 rounded-xl bg-zinc-900 border border-amber-500/30 hover:border-amber-500/50 transition-colors"
		>
			<div class="text-lg font-bold text-amber-400">Timing Display</div>
			<div class="text-sm text-zinc-400 mt-1">Full-screen timing tower</div>
		</a>

		<a
			href="/race/overlay"
			class="block p-6 rounded-xl bg-zinc-900 border border-amber-500/30 hover:border-amber-500/50 transition-colors"
		>
			<div class="text-lg font-bold text-amber-400">OBS Overlay</div>
			<div class="text-sm text-zinc-400 mt-1">Transparent browser source</div>
		</a>
	</div>
</div>
