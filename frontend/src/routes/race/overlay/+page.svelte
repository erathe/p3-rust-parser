<script lang="ts">
	import { getRaceStore } from '$lib/stores/race.svelte';
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import TimingTower from '$lib/components/race/TimingTower.svelte';

	const race = getRaceStore();

	const maxRows = $derived(Number(page.url.searchParams.get('rows')) || 8);

	const visiblePositions = $derived(
		race.positions.slice(0, maxRows)
	);

	onMount(() => {
		race.connect();
		return () => race.disconnect();
	});
</script>

<svelte:head>
	<title>OBS Overlay | P3 BMX Timing</title>
	<style>
		body {
			background: transparent !important;
		}
	</style>
</svelte:head>

<div class="fixed inset-0" style="background: transparent;">
	<div class="p-4 max-w-xl">
		<TimingTower
			phase={race.phase}
			className={race.className}
			roundType={race.roundType}
			positions={visiblePositions}
			results={race.results.slice(0, maxRows)}
			finishedCount={race.finishedCount}
			totalRiders={race.totalRiders}
		/>
	</div>
</div>
