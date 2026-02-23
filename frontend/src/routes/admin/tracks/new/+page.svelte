<script lang="ts">
	import { goto } from '$app/navigation';
	import { tracks } from '$lib/api/client';

	let name = $state('');
	let hillType = $state('8m');
	let gateBeaconId = $state(9992);
	let saving = $state(false);
	let error = $state('');

	async function handleSubmit(e: SubmitEvent) {
		e.preventDefault();
		saving = true;
		error = '';
		try {
			const track = await tracks.create({
				name,
				hill_type: hillType,
				gate_beacon_id: gateBeaconId
			});
			goto(`/admin/tracks/${track.id}`);
		} catch (err) {
			error = err instanceof Error ? err.message : 'Failed to create track';
		} finally {
			saving = false;
		}
	}
</script>

<div class="max-w-lg space-y-6">
	<div>
		<a href="/admin/tracks" class="text-sm text-zinc-500 hover:text-zinc-300">&larr; Back to tracks</a>
		<h1 class="text-2xl font-bold mt-2">New Track</h1>
	</div>

	{#if error}
		<div class="p-3 rounded-lg bg-red-500/10 text-red-400 text-sm">{error}</div>
	{/if}

	<form onsubmit={handleSubmit} class="space-y-4">
		<div>
			<label for="name" class="block text-sm font-medium text-zinc-400 mb-1">Track Name</label>
			<input
				id="name"
				type="text"
				bind:value={name}
				required
				placeholder="e.g. Sandnes BMX"
				class="w-full px-3 py-2 bg-zinc-900 border border-zinc-700 rounded-lg text-zinc-100 placeholder:text-zinc-600 focus:outline-none focus:border-amber-500/50 focus:ring-1 focus:ring-amber-500/50"
			/>
		</div>

		<div>
			<label for="hill_type" class="block text-sm font-medium text-zinc-400 mb-1">Hill Type</label>
			<select
				id="hill_type"
				bind:value={hillType}
				class="w-full px-3 py-2 bg-zinc-900 border border-zinc-700 rounded-lg text-zinc-100 focus:outline-none focus:border-amber-500/50"
			>
				<option value="5m">5 meter</option>
				<option value="8m">8 meter (Supercross)</option>
			</select>
		</div>

		<div>
			<label for="gate_beacon" class="block text-sm font-medium text-zinc-400 mb-1">
				Gate Beacon ID
			</label>
			<select
				id="gate_beacon"
				bind:value={gateBeaconId}
				class="w-full px-3 py-2 bg-zinc-900 border border-zinc-700 rounded-lg text-zinc-100 focus:outline-none focus:border-amber-500/50"
			>
				<option value={9991}>9991 (5m hill)</option>
				<option value={9992}>9992 (8m hill)</option>
				<option value={9995}>9995 (alternate)</option>
			</select>
			<p class="text-xs text-zinc-600 mt-1">
				The transponder ID used by the gate start pulse
			</p>
		</div>

		<button
			type="submit"
			disabled={saving || !name}
			class="px-4 py-2 bg-amber-500 text-black font-medium rounded-lg hover:bg-amber-400 transition-colors text-sm disabled:opacity-50 disabled:cursor-not-allowed"
		>
			{saving ? 'Creating...' : 'Create Track'}
		</button>
	</form>
</div>
