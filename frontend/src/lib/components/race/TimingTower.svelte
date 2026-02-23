<script lang="ts">
	import type { RiderPosition, FinishResult } from '$lib/api/types';
	import RaceHeader from './RaceHeader.svelte';
	import RiderRow from './RiderRow.svelte';

	let { phase, className, roundType, positions, results, finishedCount, totalRiders }: {
		phase: string;
		className: string | null;
		roundType: string | null;
		positions: RiderPosition[];
		results: FinishResult[];
		finishedCount: number;
		totalRiders: number;
	} = $props();

	const sortedPositions = $derived(
		[...positions].sort((a, b) => a.position - b.position)
	);

	function formatTime(us: number | null): string {
		if (us == null) return '--';
		const totalMs = us / 1000;
		const minutes = Math.floor(totalMs / 60000);
		const seconds = Math.floor((totalMs % 60000) / 1000);
		const ms = Math.floor(totalMs % 1000);
		if (minutes > 0) {
			return `${minutes}:${seconds.toString().padStart(2, '0')}.${ms.toString().padStart(3, '0')}`;
		}
		return `${seconds}.${ms.toString().padStart(3, '0')}`;
	}

	function formatGap(us: number | null): string {
		if (us == null || us === 0) return '';
		const totalMs = us / 1000;
		const seconds = Math.floor(totalMs / 1000);
		const ms = Math.floor(totalMs % 1000);
		return `+${seconds}.${ms.toString().padStart(3, '0')}`;
	}
</script>

<div class="flex flex-col h-full bg-zinc-950 rounded-2xl overflow-hidden border border-zinc-800/50 shadow-2xl">
	<RaceHeader {className} {roundType} {phase} {finishedCount} {totalRiders} />

	<!-- Column headers -->
	{#if sortedPositions.length > 0}
		<div class="grid grid-cols-[3rem_4rem_1fr_6rem_6rem_5rem] items-center px-4 py-1.5 bg-zinc-900/50 border-b border-zinc-800/50 text-xs font-medium text-zinc-500 uppercase tracking-wider">
			<div class="text-center">Pos</div>
			<div class="text-center">Plate</div>
			<div class="pl-3">Rider</div>
			<div class="text-right">Time</div>
			<div class="text-right">Gap</div>
			<div class="text-center">Lane</div>
		</div>
	{/if}

	<!-- Rider rows -->
	<div class="flex-1 overflow-y-auto">
		{#if phase === 'idle'}
			<div class="flex items-center justify-center h-full">
				<div class="text-center">
					<div class="text-4xl font-bold text-zinc-700 mb-2">P3</div>
					<div class="text-sm text-zinc-600">Waiting for race to be staged</div>
				</div>
			</div>
		{:else if phase === 'staged' && sortedPositions.length === 0}
			<div class="flex items-center justify-center h-full">
				<div class="text-center">
					<div class="text-2xl font-bold text-amber-400 animate-pulse mb-2">ON THE GATE</div>
					<div class="text-sm text-zinc-500">{totalRiders} riders ready</div>
				</div>
			</div>
		{:else if phase === 'finished' && results.length > 0}
			<!-- Show final results -->
			{#each results as result, i}
				<div
					class="grid grid-cols-[3rem_4rem_1fr_6rem_6rem_5rem] items-center px-4 py-2.5 {i % 2 === 0 ? 'bg-zinc-950' : 'bg-zinc-900/30'} {result.position === 1 ? 'bg-amber-500/5' : ''}"
				>
					<div class="flex items-center justify-center">
						<span class="inline-flex items-center justify-center w-8 h-8 rounded-lg text-sm font-bold
							{result.dnf ? 'bg-red-500/20 text-red-400' :
							 result.position === 1 ? 'bg-amber-500/20 text-amber-300' :
							 result.position === 2 ? 'bg-zinc-400/20 text-zinc-300' :
							 result.position === 3 ? 'bg-amber-700/20 text-amber-600' :
							 'bg-zinc-800/50 text-zinc-400'}">
							{result.dnf ? 'DNF' : result.position}
						</span>
					</div>
					<div class="flex items-center justify-center">
						<span class="inline-flex items-center justify-center min-w-[2.5rem] h-8 px-2 rounded bg-amber-400 text-zinc-950 font-bold text-sm tabular-nums">
							{result.plate_number}
						</span>
					</div>
					<div class="pl-3 truncate">
						<span class="font-semibold text-white text-sm">{result.first_name}</span>
						<span class="font-bold text-white text-sm uppercase ml-1">{result.last_name}</span>
					</div>
					<div class="text-right font-mono text-sm tabular-nums">
						{#if result.elapsed_us}
							<span class="text-white font-medium">{formatTime(result.elapsed_us)}</span>
						{:else}
							<span class="text-zinc-600">--</span>
						{/if}
					</div>
					<div class="text-right font-mono text-sm tabular-nums">
						{#if result.position === 1 && !result.dnf}
							<span class="text-amber-400 text-xs font-bold">WINNER</span>
						{:else if result.gap_to_leader_us}
							<span class="text-zinc-500">{formatGap(result.gap_to_leader_us)}</span>
						{/if}
					</div>
					<div class="text-center text-xs text-zinc-600 font-mono">&nbsp;</div>
				</div>
			{/each}
		{:else}
			{#each sortedPositions as rider, i (rider.rider_id)}
				<RiderRow {rider} index={i} isLeader={rider.position === 1 && !rider.dnf} />
			{/each}
		{/if}
	</div>

	<!-- Footer -->
	<div class="px-6 py-2 bg-zinc-900/50 border-t border-zinc-800/50 flex items-center justify-between text-xs text-zinc-600">
		<span><span class="text-amber-400 font-bold">P3</span> BMX Timing</span>
		<span class="font-mono">{new Date().toLocaleDateString()}</span>
	</div>
</div>
