<script lang="ts">
	import type { TimingLoop } from '$lib/api/types';
	import AddSection from './AddSection.svelte';

	interface EditableSection {
		name: string;
		section_type: string;
		length_m: number;
		loop_id: string | null;
	}

	let {
		sections = $bindable(),
		loops,
		selectedIndex = null,
		hillType = '8m',
		onSelect,
		onAdd,
		onRemove,
		onMove
	}: {
		sections: EditableSection[];
		loops: TimingLoop[];
		selectedIndex: number | null;
		hillType: '5m' | '8m';
		onSelect: (i: number | null) => void;
		onAdd: (name: string, sectionType: string, lengthM: number) => void;
		onRemove: (i: number) => void;
		onMove: (from: number, to: number) => void;
	} = $props();

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

	const TYPE_LABELS: Record<string, string> = {
		gate: 'Gate',
		start_hill: 'Hill',
		straight: 'Straight',
		berm: 'Berm',
		rhythm: 'Rhythm',
		tabletop: 'Table',
		pro_section: 'Pro',
		finish_straight: 'Finish',
		custom: 'Custom'
	};

	function getLoopLabel(loopId: string | null): string {
		if (!loopId) return '';
		const loop = loops.find((l) => l.id === loopId);
		return loop ? `${loop.name} (${loop.decoder_id})` : '';
	}
</script>

<div class="space-y-1.5">
	{#if sections.length === 0}
		<p class="text-sm text-zinc-500 py-4 text-center">No sections yet. Add one below or generate a default layout.</p>
	{/if}

	{#each sections as section, i}
		<!-- svelte-ignore a11y_click_events_have_key_events -->
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<div
			class="flex items-center gap-2 p-2.5 rounded-lg border transition-all cursor-pointer
				{selectedIndex === i
				? 'bg-zinc-800/80 border-amber-500/50'
				: 'bg-zinc-900 border-zinc-800 hover:border-zinc-700'}"
			onclick={() => onSelect(i)}
		>
			<!-- Position number -->
			<div class="w-5 text-center text-[10px] text-zinc-600 font-mono shrink-0">{i}</div>

			<!-- Type badge -->
			<span
				class="px-1.5 py-0.5 rounded text-[10px] font-semibold shrink-0 min-w-[3rem] text-center"
				style="background: {SECTION_COLORS[section.section_type] ?? SECTION_COLORS.custom}20;
					color: {SECTION_COLORS[section.section_type] ?? SECTION_COLORS.custom}"
			>
				{TYPE_LABELS[section.section_type] ?? section.section_type}
			</span>

			<!-- Name (editable) -->
			<input
				type="text"
				bind:value={section.name}
				onclick={(e) => e.stopPropagation()}
				class="flex-1 min-w-0 bg-transparent text-sm text-zinc-200 focus:outline-none
					border-b border-transparent focus:border-zinc-600 transition-colors"
			/>

			<!-- Length input -->
			<div class="flex items-center gap-0.5 shrink-0" onclick={(e) => e.stopPropagation()}>
				<input
					type="number"
					bind:value={section.length_m}
					step="0.5"
					min="1"
					class="w-14 bg-zinc-800 rounded px-1.5 py-0.5 text-xs text-right font-mono text-zinc-300
						focus:outline-none focus:ring-1 focus:ring-amber-500/30"
				/>
				<span class="text-[10px] text-zinc-600">m</span>
			</div>

			<!-- Decoder dropdown -->
			<div class="shrink-0" onclick={(e) => e.stopPropagation()}>
				<select
					bind:value={section.loop_id}
					class="bg-zinc-800 text-xs text-zinc-400 rounded px-1.5 py-1 border-none
						focus:outline-none focus:ring-1 focus:ring-amber-500/30 max-w-[7rem]"
				>
					<option value={null}>No decoder</option>
					{#each loops as loop}
						<option value={loop.id}>{loop.name}</option>
					{/each}
				</select>
			</div>

			<!-- Move up/down -->
			<div class="flex flex-col shrink-0" onclick={(e) => e.stopPropagation()}>
				<button
					type="button"
					onclick={() => onMove(i, i - 1)}
					disabled={i === 0}
					class="text-zinc-600 hover:text-zinc-400 disabled:opacity-30 text-xs leading-none px-0.5"
					title="Move up"
				>
					&#9650;
				</button>
				<button
					type="button"
					onclick={() => onMove(i, i + 1)}
					disabled={i === sections.length - 1}
					class="text-zinc-600 hover:text-zinc-400 disabled:opacity-30 text-xs leading-none px-0.5"
					title="Move down"
				>
					&#9660;
				</button>
			</div>

			<!-- Remove -->
			<button
				type="button"
				onclick={(e) => { e.stopPropagation(); onRemove(i); }}
				class="text-zinc-600 hover:text-red-400 transition-colors text-xs px-1 shrink-0"
				title="Remove section"
			>
				&#10005;
			</button>
		</div>
	{/each}

	<AddSection {hillType} {onAdd} />
</div>
