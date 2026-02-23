<script lang="ts">
	import type { TrackWithLoops } from '$lib/api/types';
	import { tracks } from '$lib/api/client';
	import TrackSvg from './TrackSvg.svelte';
	import SectionList from './SectionList.svelte';

	interface EditableSection {
		name: string;
		section_type: string;
		length_m: number;
		loop_id: string | null;
	}

	let {
		track,
		onUpdate
	}: {
		track: TrackWithLoops;
		onUpdate: () => void;
	} = $props();

	// Working copy of sections (component is keyed, so initial snapshot is intentional)
	function initSections(): EditableSection[] {
		return track.sections.map((s) => ({
			name: s.name,
			section_type: s.section_type,
			length_m: s.length_m,
			loop_id: s.loop_id
		}));
	}
	let sections = $state<EditableSection[]>(initSections());

	let selectedIndex = $state<number | null>(null);
	let saving = $state(false);
	let saveError = $state('');

	let totalLength = $derived(sections.reduce((sum, s) => sum + s.length_m, 0));

	// Check if sections differ from saved state
	let isDirty = $derived.by(() => {
		if (sections.length !== track.sections.length) return true;
		return sections.some(
			(s, i) =>
				s.name !== track.sections[i].name ||
				s.section_type !== track.sections[i].section_type ||
				s.length_m !== track.sections[i].length_m ||
				s.loop_id !== track.sections[i].loop_id
		);
	});

	function addSection(name: string, sectionType: string, lengthM: number) {
		sections.push({
			name,
			section_type: sectionType,
			length_m: lengthM,
			loop_id: null
		});
		// Trigger reactivity
		sections = sections;
		selectedIndex = sections.length - 1;
	}

	function removeSection(index: number) {
		sections.splice(index, 1);
		sections = sections;
		if (selectedIndex === index) selectedIndex = null;
		else if (selectedIndex !== null && selectedIndex > index) selectedIndex--;
	}

	function moveSection(from: number, to: number) {
		if (to < 0 || to >= sections.length) return;
		const [item] = sections.splice(from, 1);
		sections.splice(to, 0, item);
		sections = sections;
		selectedIndex = to;
	}

	function generateDefault() {
		const hillLength = track.hill_type === '8m' ? 8 : 5;
		sections = [
			{ name: 'Gate', section_type: 'gate', length_m: 5, loop_id: null },
			{ name: 'Start Hill', section_type: 'start_hill', length_m: hillLength, loop_id: null },
			{ name: 'First Straight', section_type: 'straight', length_m: 45, loop_id: null },
			{ name: 'Turn 1', section_type: 'berm', length_m: 22, loop_id: null },
			{ name: 'Second Straight', section_type: 'straight', length_m: 35, loop_id: null },
			{ name: 'Turn 2', section_type: 'berm', length_m: 20, loop_id: null },
			{ name: 'Third Straight', section_type: 'straight', length_m: 30, loop_id: null },
			{ name: 'Turn 3', section_type: 'berm', length_m: 18, loop_id: null },
			{ name: 'Finish Straight', section_type: 'finish_straight', length_m: 25, loop_id: null }
		];
		selectedIndex = null;
	}

	async function save() {
		saving = true;
		saveError = '';
		try {
			const input = sections.map((s) => ({
				name: s.name,
				section_type: s.section_type,
				length_m: s.length_m,
				loop_id: s.loop_id
			}));
			await tracks.saveSections(track.id, input);
			onUpdate();
		} catch (e: any) {
			saveError = e.message || 'Failed to save';
		} finally {
			saving = false;
		}
	}
</script>

<div class="space-y-4">
	<div class="flex items-center justify-between">
		<h2 class="text-lg font-semibold text-zinc-300">Track Layout</h2>
		{#if sections.length === 0}
			<button
				type="button"
				onclick={generateDefault}
				class="px-3 py-1.5 text-sm bg-amber-500 text-zinc-950 rounded-lg font-medium
					hover:bg-amber-400 transition-colors"
			>
				Generate Default Layout
			</button>
		{/if}
	</div>

	<div class="grid grid-cols-1 lg:grid-cols-2 gap-4">
		<!-- Left: SVG preview -->
		<div>
			<TrackSvg {sections} loops={track.loops} {selectedIndex} onSelect={(i) => (selectedIndex = i)} />
		</div>

		<!-- Right: Section list -->
		<div class="max-h-[400px] overflow-y-auto pr-1">
			<SectionList
				bind:sections
				loops={track.loops}
				{selectedIndex}
				hillType={track.hill_type}
				onSelect={(i) => (selectedIndex = i)}
				onAdd={addSection}
				onRemove={removeSection}
				onMove={moveSection}
			/>
		</div>
	</div>

	<!-- Footer bar -->
	<div
		class="flex items-center justify-between p-3 rounded-lg border
			{isDirty ? 'bg-zinc-900 border-amber-500/30' : 'bg-zinc-900/50 border-zinc-800'}"
	>
		<div class="flex items-center gap-4 text-sm text-zinc-400">
			<span>
				Total: <span class="font-mono text-zinc-200">{totalLength.toFixed(1)}m</span>
			</span>
			<span class="text-zinc-600">&middot;</span>
			<span>{sections.length} sections</span>
			{#if sections.filter((s) => s.loop_id).length > 0}
				<span class="text-zinc-600">&middot;</span>
				<span>{sections.filter((s) => s.loop_id).length} decoders</span>
			{/if}
		</div>

		<div class="flex items-center gap-3">
			{#if saveError}
				<span class="text-xs text-red-400">{saveError}</span>
			{/if}
			{#if isDirty}
				<span class="text-xs text-amber-500">Unsaved changes</span>
			{/if}
			<button
				type="button"
				onclick={save}
				disabled={saving || !isDirty}
				class="px-4 py-1.5 rounded-lg text-sm font-medium transition-colors
					{isDirty
					? 'bg-amber-500 text-zinc-950 hover:bg-amber-400'
					: 'bg-zinc-800 text-zinc-500'}
					disabled:opacity-50"
			>
				{saving ? 'Saving...' : 'Save Layout'}
			</button>
		</div>
	</div>
</div>
