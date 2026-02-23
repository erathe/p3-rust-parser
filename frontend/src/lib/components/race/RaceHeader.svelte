<script lang="ts">
	let { className, roundType, phase, finishedCount, totalRiders }: {
		className: string | null;
		roundType: string | null;
		phase: string;
		finishedCount: number;
		totalRiders: number;
	} = $props();

	const roundLabel = $derived(() => {
		if (!roundType) return '';
		const labels: Record<string, string> = {
			moto1: 'Moto 1',
			moto2: 'Moto 2',
			moto3: 'Moto 3',
			quarter: 'Quarter Final',
			semi: 'Semi Final',
			main: 'Main Event'
		};
		return labels[roundType] ?? roundType;
	});

	const phaseLabel = $derived(() => {
		const labels: Record<string, string> = {
			idle: 'STANDBY',
			staged: 'ON THE GATE',
			racing: 'RACING',
			finished: 'FINISHED'
		};
		return labels[phase] ?? phase.toUpperCase();
	});

	const phaseColor = $derived(() => {
		const colors: Record<string, string> = {
			idle: 'text-zinc-500',
			staged: 'text-amber-400',
			racing: 'text-green-400',
			finished: 'text-blue-400'
		};
		return colors[phase] ?? 'text-zinc-400';
	});
</script>

<div class="flex items-center justify-between px-6 py-3 bg-zinc-900/90 border-b border-zinc-800">
	<div class="flex items-center gap-4">
		{#if className}
			<h2 class="text-xl font-bold tracking-tight text-white">{className}</h2>
			<span class="text-sm font-medium text-zinc-400 bg-zinc-800 px-2.5 py-0.5 rounded">
				{roundLabel()}
			</span>
		{:else}
			<h2 class="text-xl font-bold tracking-tight text-zinc-500">No Race Loaded</h2>
		{/if}
	</div>

	<div class="flex items-center gap-4">
		{#if phase === 'racing' || phase === 'finished'}
			<span class="text-sm text-zinc-400 font-mono">
				{finishedCount}/{totalRiders} finished
			</span>
		{/if}
		<span class="text-sm font-bold tracking-wider {phaseColor()}">
			{phaseLabel()}
		</span>
	</div>
</div>
