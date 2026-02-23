<script lang="ts">
	import { getDecoderStore } from '$lib/stores/decoder.svelte';
	import { onMount } from 'svelte';

	const decoder = getDecoderStore();

	onMount(() => {
		decoder.connect();
		return () => decoder.disconnect();
	});
</script>

<div class="space-y-6">
	<div>
		<h1 class="text-2xl font-bold">Decoder Diagnostics</h1>
		<p class="text-sm text-zinc-500">Live connection status and decoder health</p>
	</div>

	<!-- Connection Status -->
	<div class="p-4 rounded-xl bg-zinc-900 border border-zinc-800">
		<div class="flex items-center gap-3">
			<div
				class="w-3 h-3 rounded-full {decoder.connected
					? 'bg-green-500 shadow-[0_0_8px_rgba(34,197,94,0.5)]'
					: 'bg-red-500 shadow-[0_0_8px_rgba(239,68,68,0.5)]'}"
			></div>
			<span class="font-medium">
				{decoder.connected ? 'Connected' : 'Disconnected'}
			</span>
			<span class="text-sm text-zinc-500 ml-auto">
				{decoder.messageCount} messages received
			</span>
		</div>
	</div>

	<!-- Decoder Status -->
	{#if decoder.lastStatus}
		<div class="grid grid-cols-2 sm:grid-cols-4 gap-4">
			<div class="p-4 rounded-xl bg-zinc-900 border border-zinc-800">
				<div class="text-xs text-zinc-500 uppercase tracking-wider">Noise</div>
				<div class="text-2xl font-bold mt-1 font-mono">{decoder.lastStatus.noise}</div>
			</div>
			<div class="p-4 rounded-xl bg-zinc-900 border border-zinc-800">
				<div class="text-xs text-zinc-500 uppercase tracking-wider">Temperature</div>
				<div class="text-2xl font-bold mt-1 font-mono">
					{(decoder.lastStatus.temperature / 10).toFixed(1)}&deg;C
				</div>
			</div>
			<div class="p-4 rounded-xl bg-zinc-900 border border-zinc-800">
				<div class="text-xs text-zinc-500 uppercase tracking-wider">GPS</div>
				<div class="text-2xl font-bold mt-1">
					{decoder.lastStatus.gps_status === 1 ? 'Locked' : 'No Fix'}
				</div>
			</div>
			<div class="p-4 rounded-xl bg-zinc-900 border border-zinc-800">
				<div class="text-xs text-zinc-500 uppercase tracking-wider">Satellites</div>
				<div class="text-2xl font-bold mt-1 font-mono">{decoder.lastStatus.satellites}</div>
			</div>
		</div>
		{#if decoder.lastStatus.decoder_id}
			<div class="text-sm text-zinc-500">
				Decoder ID: <span class="font-mono text-zinc-400">{decoder.lastStatus.decoder_id}</span>
			</div>
		{/if}
	{:else}
		<div class="text-zinc-500 text-sm">Waiting for decoder status messages...</div>
	{/if}

	<!-- Recent Passings -->
	<div>
		<h2 class="text-lg font-semibold text-zinc-300 mb-3">Recent Passings</h2>
		{#if decoder.recentPassings.length === 0}
			<p class="text-sm text-zinc-500">No passing events received yet</p>
		{:else}
			<div class="overflow-hidden rounded-xl border border-zinc-800">
				<table class="w-full text-sm">
					<thead class="bg-zinc-900">
						<tr class="text-left text-zinc-500">
							<th class="px-4 py-2 font-medium">#</th>
							<th class="px-4 py-2 font-medium">Transponder</th>
							<th class="px-4 py-2 font-medium">String</th>
							<th class="px-4 py-2 font-medium">Strength</th>
							<th class="px-4 py-2 font-medium">Hits</th>
							<th class="px-4 py-2 font-medium">Decoder</th>
						</tr>
					</thead>
					<tbody class="divide-y divide-zinc-800/50">
						{#each decoder.recentPassings.slice(0, 20) as passing}
							<tr class="hover:bg-zinc-900/50">
								<td class="px-4 py-2 font-mono text-zinc-500">{passing.passing_number}</td>
								<td class="px-4 py-2 font-mono {passing.transponder_id >= 9991 &&
								passing.transponder_id <= 9995
									? 'text-amber-400 font-bold'
									: 'text-zinc-200'}">
									{passing.transponder_id}
									{#if passing.transponder_id >= 9991 && passing.transponder_id <= 9995}
										<span class="text-xs text-amber-500/70 ml-1">GATE</span>
									{/if}
								</td>
								<td class="px-4 py-2 font-mono text-zinc-400">
									{passing.transponder_string || '-'}
								</td>
								<td class="px-4 py-2 font-mono text-zinc-400">
									{passing.strength ?? '-'}
								</td>
								<td class="px-4 py-2 font-mono text-zinc-400">{passing.hits ?? '-'}</td>
								<td class="px-4 py-2 font-mono text-zinc-500">
									{passing.decoder_id || '-'}
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{/if}
	</div>
</div>
