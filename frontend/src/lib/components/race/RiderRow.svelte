<script lang="ts">
	import type { RiderPosition } from '$lib/api/types';

	let { rider, index, isLeader }: {
		rider: RiderPosition;
		index: number;
		isLeader: boolean;
	} = $props();

	function formatTime(us: number | null): string {
		if (us == null) return '--:--.---';
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

	const positionColor = $derived(() => {
		if (rider.dnf) return 'bg-red-500/20 text-red-400';
		if (rider.position === 1) return 'bg-amber-500/20 text-amber-300';
		if (rider.position === 2) return 'bg-zinc-400/20 text-zinc-300';
		if (rider.position === 3) return 'bg-amber-700/20 text-amber-600';
		return 'bg-zinc-800/50 text-zinc-400';
	});

	const rowBg = $derived(() => {
		if (rider.finished && rider.position === 1) return 'bg-amber-500/5';
		if (rider.finished) return 'bg-zinc-900/50';
		if (rider.dnf) return 'bg-red-500/5';
		return index % 2 === 0 ? 'bg-zinc-950' : 'bg-zinc-900/30';
	});
</script>

<div
	class="grid grid-cols-[3rem_4rem_1fr_6rem_6rem_5rem] items-center px-4 py-2 transition-all duration-500 ease-out {rowBg()}"
	style="order: {rider.position};"
>
	<!-- Position -->
	<div class="flex items-center justify-center">
		<span class="inline-flex items-center justify-center w-8 h-8 rounded-lg text-sm font-bold {positionColor()}">
			{#if rider.dnf}
				DNF
			{:else}
				{rider.position}
			{/if}
		</span>
	</div>

	<!-- Plate Number -->
	<div class="flex items-center justify-center">
		<span class="inline-flex items-center justify-center min-w-[2.5rem] h-8 px-2 rounded bg-amber-400 text-zinc-950 font-bold text-sm tabular-nums">
			{rider.plate_number}
		</span>
	</div>

	<!-- Name -->
	<div class="pl-3 truncate">
		<span class="font-semibold text-white text-sm">{rider.first_name}</span>
		<span class="font-bold text-white text-sm uppercase ml-1">{rider.last_name}</span>
		{#if rider.last_loop && !rider.finished}
			<span class="text-xs text-zinc-500 ml-2">{rider.last_loop}</span>
		{/if}
	</div>

	<!-- Time -->
	<div class="text-right font-mono text-sm tabular-nums">
		{#if rider.finished}
			<span class="text-white font-medium">{formatTime(rider.elapsed_us)}</span>
		{:else if rider.elapsed_us}
			<span class="text-zinc-400">{formatTime(rider.elapsed_us)}</span>
		{:else}
			<span class="text-zinc-600">--:--.---</span>
		{/if}
	</div>

	<!-- Gap -->
	<div class="text-right font-mono text-sm tabular-nums">
		{#if isLeader}
			<span class="text-amber-400 text-xs font-bold">LEADER</span>
		{:else if rider.gap_to_leader_us}
			<span class="text-zinc-500">{formatGap(rider.gap_to_leader_us)}</span>
		{/if}
	</div>

	<!-- Lane -->
	<div class="text-center text-xs text-zinc-600 font-mono">
		L{rider.lane}
	</div>
</div>
