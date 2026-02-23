<script lang="ts">
	import { getRaceStore } from '$lib/stores/race.svelte';
	import { onMount } from 'svelte';
	import TimingTower from '$lib/components/race/TimingTower.svelte';

	const race = getRaceStore();

	onMount(() => {
		race.connect();
		return () => race.disconnect();
	});
</script>

<svelte:head>
	<title>Race Display | P3 BMX Timing</title>
</svelte:head>

<div class="fixed inset-0 bg-zinc-950 p-4">
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
