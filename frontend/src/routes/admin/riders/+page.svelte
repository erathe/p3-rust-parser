<script lang="ts">
	import { riders } from '$lib/api/client';
	import type { Rider, CreateRiderRequest } from '$lib/api/types';

	let riderList = $state<Rider[]>([]);
	let loading = $state(true);
	let search = $state('');
	let showForm = $state(false);
	let editingId = $state<string | null>(null);
	let saving = $state(false);
	let error = $state('');

	// Form fields
	let firstName = $state('');
	let lastName = $state('');
	let plateNumber = $state('');
	let transponderId = $state(0);
	let transponderString = $state('');
	let skillLevel = $state('');
	let gender = $state('');

	$effect(() => {
		loadRiders();
	});

	async function loadRiders() {
		loading = true;
		riderList = await riders.list(search || undefined);
		loading = false;
	}

	function resetForm() {
		firstName = '';
		lastName = '';
		plateNumber = '';
		transponderId = 0;
		transponderString = '';
		skillLevel = '';
		gender = '';
		editingId = null;
		showForm = false;
		error = '';
	}

	function editRider(rider: Rider) {
		firstName = rider.first_name;
		lastName = rider.last_name;
		plateNumber = rider.plate_number;
		transponderId = rider.transponder_id;
		transponderString = rider.transponder_string || '';
		skillLevel = rider.skill_level || '';
		gender = rider.gender || '';
		editingId = rider.id;
		showForm = true;
	}

	async function handleSubmit(e: SubmitEvent) {
		e.preventDefault();
		saving = true;
		error = '';

		const data: CreateRiderRequest = {
			first_name: firstName,
			last_name: lastName,
			plate_number: plateNumber,
			transponder_id: transponderId,
			transponder_string: transponderString || null,
			skill_level: skillLevel || null,
			gender: gender || null
		};

		try {
			if (editingId) {
				await riders.update(editingId, data);
			} else {
				await riders.create(data);
			}
			resetForm();
			await loadRiders();
		} catch (err) {
			error = err instanceof Error ? err.message : 'Failed to save rider';
		} finally {
			saving = false;
		}
	}

	async function handleDelete(id: string) {
		if (!confirm('Delete this rider?')) return;
		await riders.delete(id);
		await loadRiders();
	}

	let searchTimeout: ReturnType<typeof setTimeout>;
	function handleSearch(value: string) {
		search = value;
		clearTimeout(searchTimeout);
		searchTimeout = setTimeout(loadRiders, 300);
	}
</script>

<div class="space-y-6">
	<div class="flex items-center justify-between">
		<div>
			<h1 class="text-2xl font-bold">Riders</h1>
			<p class="text-sm text-zinc-500">Manage rider registrations and transponder assignments</p>
		</div>
		<button
			onclick={() => {
				resetForm();
				showForm = true;
			}}
			class="px-4 py-2 bg-amber-500 text-black font-medium rounded-lg hover:bg-amber-400 transition-colors text-sm"
		>
			Add Rider
		</button>
	</div>

	<!-- Search -->
	<input
		type="text"
		placeholder="Search by name, plate, or transponder..."
		value={search}
		oninput={(e) => handleSearch(e.currentTarget.value)}
		class="w-full px-3 py-2 bg-zinc-900 border border-zinc-800 rounded-lg text-sm text-zinc-100 placeholder:text-zinc-600 focus:outline-none focus:border-amber-500/50"
	/>

	{#if error}
		<div class="p-3 rounded-lg bg-red-500/10 text-red-400 text-sm">{error}</div>
	{/if}

	<!-- Add/Edit Form -->
	{#if showForm}
		<form
			onsubmit={handleSubmit}
			class="p-4 rounded-xl bg-zinc-900 border border-zinc-800 space-y-4"
		>
			<h2 class="font-semibold text-zinc-300">
				{editingId ? 'Edit Rider' : 'New Rider'}
			</h2>
			<div class="grid grid-cols-2 gap-3">
				<div>
					<label class="block text-xs text-zinc-500 mb-1">First Name</label>
					<input
						type="text"
						bind:value={firstName}
						required
						class="w-full px-3 py-1.5 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-100 focus:outline-none focus:border-amber-500/50"
					/>
				</div>
				<div>
					<label class="block text-xs text-zinc-500 mb-1">Last Name</label>
					<input
						type="text"
						bind:value={lastName}
						required
						class="w-full px-3 py-1.5 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-100 focus:outline-none focus:border-amber-500/50"
					/>
				</div>
				<div>
					<label class="block text-xs text-zinc-500 mb-1">Plate Number</label>
					<input
						type="text"
						bind:value={plateNumber}
						required
						class="w-full px-3 py-1.5 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-100 focus:outline-none focus:border-amber-500/50"
					/>
				</div>
				<div>
					<label class="block text-xs text-zinc-500 mb-1">Transponder ID</label>
					<input
						type="number"
						bind:value={transponderId}
						required
						class="w-full px-3 py-1.5 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-100 font-mono focus:outline-none focus:border-amber-500/50"
					/>
				</div>
				<div>
					<label class="block text-xs text-zinc-500 mb-1">Transponder String</label>
					<input
						type="text"
						bind:value={transponderString}
						placeholder="e.g. FL-94890"
						class="w-full px-3 py-1.5 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-100 font-mono placeholder:text-zinc-600 focus:outline-none focus:border-amber-500/50"
					/>
				</div>
				<div>
					<label class="block text-xs text-zinc-500 mb-1">Skill Level</label>
					<select
						bind:value={skillLevel}
						class="w-full px-3 py-1.5 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-100 focus:outline-none focus:border-amber-500/50"
					>
						<option value="">-- Select --</option>
						<option value="Novice">Novice</option>
						<option value="Intermediate">Intermediate</option>
						<option value="Expert">Expert</option>
					</select>
				</div>
				<div>
					<label class="block text-xs text-zinc-500 mb-1">Gender</label>
					<select
						bind:value={gender}
						class="w-full px-3 py-1.5 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-100 focus:outline-none focus:border-amber-500/50"
					>
						<option value="">-- Select --</option>
						<option value="Male">Male</option>
						<option value="Female">Female</option>
					</select>
				</div>
			</div>
			<div class="flex gap-2">
				<button
					type="submit"
					disabled={saving}
					class="px-4 py-2 bg-amber-500 text-black font-medium rounded-lg hover:bg-amber-400 transition-colors text-sm disabled:opacity-50"
				>
					{saving ? 'Saving...' : editingId ? 'Update' : 'Add Rider'}
				</button>
				<button
					type="button"
					onclick={resetForm}
					class="px-4 py-2 bg-zinc-800 text-zinc-300 rounded-lg hover:bg-zinc-700 transition-colors text-sm"
				>
					Cancel
				</button>
			</div>
		</form>
	{/if}

	<!-- Rider Table -->
	{#if loading}
		<div class="text-zinc-500">Loading...</div>
	{:else if riderList.length === 0}
		<div class="text-center py-12 text-zinc-500">
			{search ? 'No riders match your search' : 'No riders registered yet'}
		</div>
	{:else}
		<div class="overflow-hidden rounded-xl border border-zinc-800">
			<table class="w-full text-sm">
				<thead class="bg-zinc-900">
					<tr class="text-left text-zinc-500">
						<th class="px-4 py-3 font-medium">Plate</th>
						<th class="px-4 py-3 font-medium">Name</th>
						<th class="px-4 py-3 font-medium">Transponder</th>
						<th class="px-4 py-3 font-medium">Level</th>
						<th class="px-4 py-3 font-medium w-24"></th>
					</tr>
				</thead>
				<tbody class="divide-y divide-zinc-800/50">
					{#each riderList as rider}
						<tr class="hover:bg-zinc-900/50">
							<td class="px-4 py-3 font-mono font-bold text-amber-400">{rider.plate_number}</td>
							<td class="px-4 py-3 text-zinc-200">
								{rider.first_name}
								{rider.last_name}
							</td>
							<td class="px-4 py-3 font-mono text-zinc-500">
								{rider.transponder_string || rider.transponder_id}
							</td>
							<td class="px-4 py-3 text-zinc-500">{rider.skill_level || '-'}</td>
							<td class="px-4 py-3 text-right">
								<button
									class="text-xs text-zinc-500 hover:text-zinc-300 mr-2"
									onclick={() => editRider(rider)}
								>
									Edit
								</button>
								<button
									class="text-xs text-red-400 hover:text-red-300"
									onclick={() => handleDelete(rider.id)}
								>
									Delete
								</button>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{/if}
</div>
