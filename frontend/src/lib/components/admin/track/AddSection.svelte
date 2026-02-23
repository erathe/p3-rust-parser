<script lang="ts">
	let {
		hillType = '8m',
		onAdd
	}: {
		hillType: '5m' | '8m';
		onAdd: (name: string, sectionType: string, lengthM: number) => void;
	} = $props();

	let open = $state(false);
	let customName = $state('');
	let customLength = $state(15);

	const PRESETS = $derived([
		{ type: 'gate', name: 'Gate', length: 5 },
		{ type: 'start_hill', name: 'Start Hill', length: hillType === '8m' ? 8 : 5 },
		{ type: 'straight', name: 'Straight', length: 40 },
		{ type: 'berm', name: 'Berm / Turn', length: 20 },
		{ type: 'rhythm', name: 'Rhythm Section', length: 30 },
		{ type: 'tabletop', name: 'Tabletop', length: 12 },
		{ type: 'pro_section', name: 'Pro Section', length: 20 },
		{ type: 'finish_straight', name: 'Finish Straight', length: 25 }
	]);

	const SECTION_COLORS: Record<string, string> = {
		gate: '#f59e0b',
		start_hill: '#22c55e',
		straight: '#6b7280',
		berm: '#ef4444',
		rhythm: '#8b5cf6',
		tabletop: '#3b82f6',
		pro_section: '#ec4899',
		finish_straight: '#f97316',
		custom: '#14b8a6'
	};

	function addPreset(preset: { type: string; name: string; length: number }) {
		onAdd(preset.name, preset.type, preset.length);
		open = false;
	}

	function addCustom() {
		if (!customName.trim()) return;
		onAdd(customName.trim(), 'custom', customLength);
		customName = '';
		customLength = 15;
		open = false;
	}
</script>

<div class="relative">
	<button
		type="button"
		onclick={() => (open = !open)}
		class="w-full py-2 px-3 rounded-lg border border-dashed border-zinc-700 text-sm text-zinc-500
			hover:border-zinc-600 hover:text-zinc-400 transition-colors"
	>
		+ Add Section
	</button>

	{#if open}
		<!-- svelte-ignore a11y_click_events_have_key_events -->
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<div class="fixed inset-0 z-40" onclick={() => (open = false)}></div>
		<div
			class="absolute bottom-full left-0 right-0 mb-1 z-50 bg-zinc-900 border border-zinc-700
				rounded-lg shadow-xl overflow-hidden max-h-80 overflow-y-auto"
		>
			<div class="p-2 text-xs text-zinc-500 font-medium uppercase tracking-wider">
				Predefined
			</div>
			{#each PRESETS as preset}
				<button
					type="button"
					onclick={() => addPreset(preset)}
					class="w-full flex items-center gap-2 px-3 py-2 text-sm text-left hover:bg-zinc-800 transition-colors"
				>
					<span
						class="w-3 h-3 rounded-sm shrink-0"
						style="background: {SECTION_COLORS[preset.type]}"
					></span>
					<span class="flex-1 text-zinc-300">{preset.name}</span>
					<span class="text-xs text-zinc-500 font-mono">{preset.length}m</span>
				</button>
			{/each}

			<div class="border-t border-zinc-800 p-2 text-xs text-zinc-500 font-medium uppercase tracking-wider">
				Custom
			</div>
			<form onsubmit={(e) => { e.preventDefault(); addCustom(); }} class="flex items-center gap-2 px-3 py-2">
				<span
					class="w-3 h-3 rounded-sm shrink-0"
					style="background: {SECTION_COLORS.custom}"
				></span>
				<input
					type="text"
					bind:value={customName}
					placeholder="Section name"
					class="flex-1 bg-zinc-800 rounded px-2 py-1 text-sm text-zinc-200 placeholder:text-zinc-600
						focus:outline-none focus:ring-1 focus:ring-amber-500/50"
				/>
				<input
					type="number"
					bind:value={customLength}
					min="1"
					step="0.5"
					class="w-16 bg-zinc-800 rounded px-2 py-1 text-sm text-right font-mono text-zinc-200
						focus:outline-none focus:ring-1 focus:ring-amber-500/50"
				/>
				<span class="text-xs text-zinc-500">m</span>
				<button
					type="submit"
					disabled={!customName.trim()}
					class="px-2 py-1 bg-zinc-700 text-zinc-300 rounded text-xs hover:bg-zinc-600 disabled:opacity-50"
				>
					Add
				</button>
			</form>
		</div>
	{/if}
</div>
