<script lang="ts">
	import { page } from '$app/stores';
	import { events as eventsApi, riders as ridersApi } from '$lib/api/client';
	import type { EventWithClasses, Rider, Moto } from '$lib/api/types';
	import { onMount } from 'svelte';

	let event = $state<EventWithClasses | null>(null);
	let allRiders = $state<Rider[]>([]);
	let classMotos = $state<Record<string, Moto[]>>({});
	let loading = $state(true);
	let error = $state('');

	// Class form
	let showClassForm = $state(false);
	let newClassName = $state('');
	let saving = $state(false);

	// Rider add
	let addingRiderClassId = $state('');
	let selectedRiderId = $state('');

	const eventId = $derived($page.params.id!);

	$effect(() => {
		loadEvent();
	});

	async function loadEvent() {
		loading = true;
		try {
			event = await eventsApi.get(eventId);
			allRiders = await ridersApi.list();
			// Load motos for each class
			const motoMap: Record<string, Moto[]> = {};
			for (const cls of event.classes) {
				try {
					motoMap[cls.id] = await eventsApi.listClassMotos(eventId, cls.id);
				} catch {
					motoMap[cls.id] = [];
				}
			}
			classMotos = motoMap;
		} catch (e: any) {
			error = e.message;
		}
		loading = false;
	}

	async function addClass(e: SubmitEvent) {
		e.preventDefault();
		if (!newClassName) return;
		saving = true;
		try {
			await eventsApi.createClass(eventId, { name: newClassName });
			newClassName = '';
			showClassForm = false;
			await loadEvent();
		} catch (err: any) {
			error = err.message;
		}
		saving = false;
	}

	async function deleteClass(classId: string) {
		if (!confirm('Delete this class and all its motos?')) return;
		try {
			await eventsApi.deleteClass(eventId, classId);
			await loadEvent();
		} catch (err: any) {
			error = err.message;
		}
	}

	async function addRider(classId: string) {
		if (!selectedRiderId) return;
		try {
			await eventsApi.addClassRider(eventId, classId, selectedRiderId);
			selectedRiderId = '';
			addingRiderClassId = '';
			await loadEvent();
		} catch (err: any) {
			error = err.message;
		}
	}

	async function removeRider(classId: string, riderId: string) {
		try {
			await eventsApi.removeClassRider(eventId, classId, riderId);
			await loadEvent();
		} catch (err: any) {
			error = err.message;
		}
	}

	async function generateMotos(classId: string) {
		try {
			const result = await eventsApi.generateMotos(eventId, classId);
			error = '';
			await loadEvent();
		} catch (err: any) {
			error = err.message;
		}
	}

	function roundTypeLabel(rt: string): string {
		const labels: Record<string, string> = {
			moto1: 'Moto 1', moto2: 'Moto 2', moto3: 'Moto 3',
			quarter: 'Quarter', semi: 'Semi', main: 'Main'
		};
		return labels[rt] ?? rt;
	}
</script>

<div class="space-y-6">
	<a href="/admin/events" class="text-sm text-zinc-500 hover:text-zinc-300">&larr; Back to events</a>

	{#if loading}
		<div class="text-zinc-500">Loading...</div>
	{:else if !event}
		<div class="text-red-400">Event not found</div>
	{:else}
		<div class="flex items-center justify-between">
			<div>
				<h1 class="text-2xl font-bold">{event.name}</h1>
				<p class="text-sm text-zinc-500">{event.date}</p>
			</div>
			<button
				onclick={() => (showClassForm = !showClassForm)}
				class="px-3 py-1.5 bg-zinc-700 text-zinc-200 rounded-lg text-sm hover:bg-zinc-600"
			>
				{showClassForm ? 'Cancel' : 'Add Class'}
			</button>
		</div>

		{#if error}
			<div class="p-3 rounded-lg bg-red-500/10 text-red-400 text-sm">{error}</div>
		{/if}

		{#if showClassForm}
			<form onsubmit={addClass} class="p-4 rounded-xl bg-zinc-900 border border-zinc-800 flex gap-3 items-end">
				<div class="flex-1">
					<label for="class_name" class="block text-xs text-zinc-500 mb-1">Class Name</label>
					<input
						id="class_name"
						type="text"
						bind:value={newClassName}
						required
						placeholder="e.g. Novice 9-10"
						class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm"
					/>
				</div>
				<button
					type="submit"
					disabled={saving}
					class="px-4 py-2 bg-amber-500 text-zinc-950 rounded-lg font-medium text-sm hover:bg-amber-400 disabled:opacity-50"
				>
					{saving ? 'Adding...' : 'Add'}
				</button>
			</form>
		{/if}

		{#if event.classes.length === 0}
			<div class="text-center py-8 text-zinc-500">
				<p>No classes yet. Add a class to start configuring riders.</p>
			</div>
		{/if}

		{#each event.classes as cls}
			<div class="rounded-xl bg-zinc-900 border border-zinc-800 overflow-hidden">
				<div class="p-4 flex items-center justify-between border-b border-zinc-800">
					<div>
						<h3 class="font-semibold text-zinc-200">{cls.name}</h3>
						<p class="text-xs text-zinc-500">{cls.riders.length} riders &middot; {cls.race_format}</p>
					</div>
					<div class="flex gap-2">
						<button
							onclick={() => generateMotos(cls.id)}
							disabled={cls.riders.length === 0}
							class="px-3 py-1.5 text-xs font-medium rounded-lg transition-colors
								{cls.riders.length === 0
									? 'bg-zinc-800 text-zinc-600 cursor-not-allowed'
									: 'bg-amber-500/10 text-amber-400 hover:bg-amber-500/20'}"
						>
							Generate Motos
						</button>
						<button
							onclick={() => deleteClass(cls.id)}
							class="text-xs text-red-400 hover:bg-red-500/10 px-2 py-1 rounded"
						>
							Delete
						</button>
					</div>
				</div>

				<!-- Riders in class -->
				<div class="p-4 space-y-2">
					{#each cls.riders as rider}
						<div class="flex items-center gap-3 text-sm">
							<span class="inline-flex items-center justify-center min-w-[2rem] h-6 px-1.5 rounded bg-amber-400 text-zinc-950 font-bold text-xs">
								{rider.plate_number}
							</span>
							<span class="text-zinc-300">{rider.first_name} {rider.last_name}</span>
							<span class="text-xs text-zinc-600 font-mono">T:{rider.transponder_id}</span>
							<button
								onclick={() => removeRider(cls.id, rider.id)}
								class="ml-auto text-xs text-zinc-600 hover:text-red-400"
							>
								Remove
							</button>
						</div>
					{/each}

					{#if addingRiderClassId === cls.id}
						<div class="flex gap-2 items-center mt-2">
							<select
								bind:value={selectedRiderId}
								class="flex-1 px-2 py-1.5 bg-zinc-800 border border-zinc-700 rounded-lg text-sm"
							>
								<option value="">Select rider...</option>
								{#each allRiders.filter((r) => !cls.riders.some((cr) => cr.id === r.id)) as rider}
									<option value={rider.id}>{rider.plate_number} — {rider.first_name} {rider.last_name}</option>
								{/each}
							</select>
							<button
								onclick={() => addRider(cls.id)}
								disabled={!selectedRiderId}
								class="px-3 py-1.5 bg-zinc-700 text-zinc-200 rounded-lg text-xs hover:bg-zinc-600 disabled:opacity-50"
							>
								Add
							</button>
							<button
								onclick={() => (addingRiderClassId = '')}
								class="text-xs text-zinc-500"
							>
								Cancel
							</button>
						</div>
					{:else}
						<button
							onclick={() => (addingRiderClassId = cls.id)}
							class="text-xs text-zinc-500 hover:text-zinc-300 mt-1"
						>
							+ Add rider
						</button>
					{/if}
				</div>

				<!-- Motos -->
				{#if classMotos[cls.id]?.length > 0}
					<div class="border-t border-zinc-800 p-4">
						<h4 class="text-xs font-medium text-zinc-500 uppercase tracking-wider mb-2">Motos</h4>
						<div class="grid grid-cols-3 sm:grid-cols-4 md:grid-cols-6 gap-2">
							{#each classMotos[cls.id] as moto}
								<div class="p-2 rounded-lg bg-zinc-800/50 text-center text-xs">
									<div class="font-medium text-zinc-300">
										{roundTypeLabel(moto.round_type)}
										{moto.round_number ? `#${moto.round_number}` : ''}
									</div>
									<div class="text-zinc-600 mt-0.5">{moto.status}</div>
								</div>
							{/each}
						</div>
					</div>
				{/if}
			</div>
		{/each}
	{/if}
</div>
