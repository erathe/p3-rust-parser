<script lang="ts">
	import { page } from '$app/stores';
	import { onDestroy, onMount } from 'svelte';
	import { onboarding, tracks } from '$lib/api/client';
	import type { DiscoveredDecoder, TrackOnboardingDiscoveryResponse, TrackWithLoops } from '$lib/api/types';

	type LoopRole = 'start' | 'split' | 'finish';
	type DecoderDraft = { name: string; role: LoopRole };

	const trackId = $derived($page.params.id!);

	let track = $state<TrackWithLoops | null>(null);
	let loading = $state(true);
	let error = $state('');

	let discovery = $state<TrackOnboardingDiscoveryResponse | null>(null);
	let discoveryLoading = $state(false);
	let listening = $state(true);
	let windowSeconds = $state(180);

	let locationLabel = $state('');
	let timezone = $state('');
	let latitude = $state<number | null>(null);
	let longitude = $state<number | null>(null);
	let savingLocation = $state(false);
	let locationMessage = $state('');

	let drafts = $state<Record<string, DecoderDraft>>({});
	let addingDecoderId = $state<string | null>(null);

	const MAP_WIDTH = 760;
	const MAP_HEIGHT = 320;
	const POLL_INTERVAL_MS = 3000;
	let pollingTimer: ReturnType<typeof setInterval> | null = null;

	let markerX = $derived.by(() =>
		longitude === null ? null : Number((((longitude + 180) / 360) * MAP_WIDTH).toFixed(2))
	);
	let markerY = $derived.by(() =>
		latitude === null ? null : Number((((90 - latitude) / 180) * MAP_HEIGHT).toFixed(2))
	);

	let locationDirty = $derived.by(() => {
		if (!track) return false;
		return (
			locationLabel !== (track.location_label ?? '') ||
			timezone !== (track.timezone ?? '') ||
			latitude !== track.latitude ||
			longitude !== track.longitude
		);
	});

	onMount(async () => {
		await loadTrack();
		await refreshDiscovery();
		if (listening) {
			startPolling();
		}
	});

	onDestroy(() => {
		stopPolling();
	});

	async function loadTrack() {
		loading = true;
		error = '';
		try {
			const data = await tracks.get(trackId);
			track = data;
			locationLabel = data.location_label ?? '';
			timezone = data.timezone ?? '';
			latitude = data.latitude;
			longitude = data.longitude;
		} catch (e: any) {
			error = e.message ?? 'Failed to load track';
		} finally {
			loading = false;
		}
	}

	function stopPolling() {
		if (pollingTimer) {
			clearInterval(pollingTimer);
			pollingTimer = null;
		}
	}

	function startPolling() {
		stopPolling();
		pollingTimer = setInterval(() => {
			void refreshDiscovery(true);
		}, POLL_INTERVAL_MS);
	}

	function setListening(enabled: boolean) {
		listening = enabled;
		if (enabled) {
			startPolling();
			void refreshDiscovery();
		} else {
			stopPolling();
		}
	}

	function updateWindow(value: string) {
		const parsed = Number(value);
		if (!Number.isFinite(parsed)) return;
		windowSeconds = parsed;
		void refreshDiscovery();
		if (listening) {
			startPolling();
		}
	}

	async function refreshDiscovery(background = false) {
		if (discoveryLoading && background) return;
		if (!background) discoveryLoading = true;
		try {
			const data = await onboarding.discovery(trackId, {
				window_seconds: windowSeconds,
				max_messages: 5000
			});
			discovery = data;
			bootstrapDrafts(data.decoders);
		} catch (e: any) {
			if (!background) error = e.message ?? 'Failed to load discovery data';
		} finally {
			if (!background) discoveryLoading = false;
		}
	}

	function parseOptionalNumber(value: string): number | null {
		const normalized = value.trim();
		if (normalized === '') return null;
		const parsed = Number(normalized);
		return Number.isFinite(parsed) ? parsed : null;
	}

	function updateLatitude(value: string) {
		latitude = parseOptionalNumber(value);
	}

	function updateLongitude(value: string) {
		longitude = parseOptionalNumber(value);
	}

	function mapClick(event: MouseEvent) {
		const svg = event.currentTarget as SVGSVGElement;
		const rect = svg.getBoundingClientRect();
		const x = Math.min(Math.max(event.clientX - rect.left, 0), rect.width);
		const y = Math.min(Math.max(event.clientY - rect.top, 0), rect.height);
		longitude = Number((((x / rect.width) * 360) - 180).toFixed(6));
		latitude = Number((90 - (y / rect.height) * 180).toFixed(6));
	}

	async function saveLocation() {
		if (!track) return;
		if ((latitude === null) !== (longitude === null)) {
			locationMessage = 'Latitude and longitude must be set together.';
			return;
		}

		savingLocation = true;
		locationMessage = '';
		try {
			await tracks.update(track.id, {
				name: track.name,
				hill_type: track.hill_type,
				gate_beacon_id: track.gate_beacon_id,
				location_label: locationLabel.trim() || null,
				timezone: timezone.trim() || null,
				latitude,
				longitude
			});
			locationMessage = 'Location saved.';
			await loadTrack();
		} catch (e: any) {
			locationMessage = e.message ?? 'Failed to save location';
		} finally {
			savingLocation = false;
		}
	}

	function suggestedRole(decoder: DiscoveredDecoder): LoopRole {
		if (decoder.mapped_role) return decoder.mapped_role;
		if (decoder.gate_hits > 0) return 'start';
		return 'split';
	}

	function defaultName(role: LoopRole): string {
		if (!track) {
			return role === 'start' ? 'Start Hill' : role === 'finish' ? 'Finish' : 'Split';
		}

		if (role === 'start') return 'Start Hill';
		if (role === 'finish') return 'Finish';
		const splitCount = track.loops.filter((l) => !l.is_start && !l.is_finish).length;
		return `Split ${splitCount + 1}`;
	}

	function bootstrapDrafts(decoders: DiscoveredDecoder[]) {
		const next = { ...drafts };
		for (const decoder of decoders) {
			if (next[decoder.decoder_id]) continue;
			const role = suggestedRole(decoder);
			next[decoder.decoder_id] = {
				role,
				name: defaultName(role)
			};
		}
		drafts = next;
	}

	function updateDraftName(decoderId: string, value: string) {
		const current = drafts[decoderId] ?? { role: 'split' as LoopRole, name: defaultName('split') };
		drafts = {
			...drafts,
			[decoderId]: {
				...current,
				name: value
			}
		};
	}

	function updateDraftRole(decoderId: string, role: LoopRole) {
		const current = drafts[decoderId] ?? { role, name: defaultName(role) };
		const nextName = current.name.trim() === '' || current.name === defaultName(current.role)
			? defaultName(role)
			: current.name;
		drafts = {
			...drafts,
			[decoderId]: {
				role,
				name: nextName
			}
		};
	}

	function rolePill(role: string | null): string {
		if (role === 'start') return 'text-green-300 bg-green-500/20';
		if (role === 'finish') return 'text-red-300 bg-red-500/20';
		if (role === 'split') return 'text-blue-300 bg-blue-500/20';
		return 'text-zinc-400 bg-zinc-800';
	}

	function formatUtcTimestamp(value: string): string {
		const iso = value.includes('T') ? value : value.replace(' ', 'T');
		const parsed = new Date(`${iso}Z`);
		return Number.isNaN(parsed.getTime()) ? value : parsed.toLocaleString();
	}

	async function addLoopFromDecoder(decoder: DiscoveredDecoder) {
		if (!track) return;
		const draft = drafts[decoder.decoder_id];
		if (!draft) return;

		addingDecoderId = decoder.decoder_id;
		error = '';
		try {
			await tracks.createLoop(track.id, {
				name: draft.name.trim() || defaultName(draft.role),
				decoder_id: decoder.decoder_id,
				position: track.loops.length,
				is_start: draft.role === 'start',
				is_finish: draft.role === 'finish'
			});
			await loadTrack();
			await refreshDiscovery();
		} catch (e: any) {
			error = e.message ?? `Failed to map decoder ${decoder.decoder_id}`;
		} finally {
			addingDecoderId = null;
		}
	}
</script>

<div class="space-y-6">
	<a href="/admin/tracks/{trackId}" class="text-sm text-zinc-500 hover:text-zinc-300">
		&larr; Back to track
	</a>

	{#if loading}
		<div class="text-zinc-500">Loading onboarding workspace...</div>
	{:else if !track}
		<div class="text-red-400">Track not found</div>
	{:else}
		<div>
			<h1 class="text-2xl font-bold">Track Onboarding</h1>
			<p class="text-sm text-zinc-500 mt-1">
				{track.name} &middot; Track-scoped decoder discovery and location setup
			</p>
		</div>

		{#if error}
			<div class="p-3 rounded-lg bg-red-500/10 text-red-400 text-sm">{error}</div>
		{/if}

		<div class="grid grid-cols-1 xl:grid-cols-[1.15fr_1fr] gap-6">
			<!-- Location card -->
			<div class="space-y-4 p-4 rounded-xl bg-zinc-900 border border-zinc-800">
				<div class="flex items-center justify-between">
					<div>
						<h2 class="text-lg font-semibold text-zinc-200">Track location</h2>
						<p class="text-xs text-zinc-500">Click the map or enter coordinates manually.</p>
					</div>
					{#if latitude !== null && longitude !== null}
						<a
							href="https://www.openstreetmap.org/?mlat={latitude}&mlon={longitude}#map=15/{latitude}/{longitude}"
							target="_blank"
							rel="noreferrer"
							class="text-xs text-amber-400 hover:text-amber-300"
						>
							Open in OSM
						</a>
					{/if}
				</div>

				<div class="rounded-lg border border-zinc-700 overflow-hidden bg-zinc-950">
					<!-- svelte-ignore a11y_click_events_have_key_events -->
					<!-- svelte-ignore a11y_no_static_element_interactions -->
					<svg
						viewBox="0 0 {MAP_WIDTH} {MAP_HEIGHT}"
						class="w-full h-[280px] cursor-crosshair"
						onclick={mapClick}
					>
						<rect x="0" y="0" width={MAP_WIDTH} height={MAP_HEIGHT} fill="#0f172a" />

						{#each [30, 60, 90, 120, 150, 180, 210, 240, 270, 300, 330] as x}
							<line x1={x * (MAP_WIDTH / 360)} y1="0" x2={x * (MAP_WIDTH / 360)} y2={MAP_HEIGHT} stroke="#1f2937" stroke-width="1" />
						{/each}
						{#each [30, 60, 90, 120, 150] as y}
							<line x1="0" y1={y * (MAP_HEIGHT / 180)} x2={MAP_WIDTH} y2={y * (MAP_HEIGHT / 180)} stroke="#1f2937" stroke-width="1" />
						{/each}

						<!-- Equator and prime meridian -->
						<line x1="0" y1={MAP_HEIGHT / 2} x2={MAP_WIDTH} y2={MAP_HEIGHT / 2} stroke="#334155" stroke-width="1.5" />
						<line x1={MAP_WIDTH / 2} y1="0" x2={MAP_WIDTH / 2} y2={MAP_HEIGHT} stroke="#334155" stroke-width="1.5" />

						{#if markerX !== null && markerY !== null}
							<circle cx={markerX} cy={markerY} r="6" fill="#f59e0b" stroke="#111827" stroke-width="2" />
							<circle cx={markerX} cy={markerY} r="14" fill="none" stroke="#f59e0b" stroke-opacity="0.35" stroke-width="2" />
						{/if}
					</svg>
				</div>

				<div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
					<div>
						<label for="location_label" class="block text-xs text-zinc-500 mb-1">Location label</label>
						<input
							id="location_label"
							type="text"
							bind:value={locationLabel}
							placeholder="e.g. Sandnes, Norway"
							class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm"
						/>
					</div>
					<div>
						<label for="timezone" class="block text-xs text-zinc-500 mb-1">Timezone</label>
						<input
							id="timezone"
							type="text"
							bind:value={timezone}
							placeholder="e.g. Europe/Oslo"
							class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm"
						/>
					</div>
					<div>
						<label for="latitude" class="block text-xs text-zinc-500 mb-1">Latitude</label>
						<input
							id="latitude"
							type="number"
							step="0.000001"
							min="-90"
							max="90"
							value={latitude ?? ''}
							oninput={(e) => updateLatitude((e.currentTarget as HTMLInputElement).value)}
							class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm font-mono"
						/>
					</div>
					<div>
						<label for="longitude" class="block text-xs text-zinc-500 mb-1">Longitude</label>
						<input
							id="longitude"
							type="number"
							step="0.000001"
							min="-180"
							max="180"
							value={longitude ?? ''}
							oninput={(e) => updateLongitude((e.currentTarget as HTMLInputElement).value)}
							class="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm font-mono"
						/>
					</div>
				</div>

				<div class="flex items-center gap-3">
					<button
						type="button"
						onclick={saveLocation}
						disabled={!locationDirty || savingLocation}
						class="px-4 py-2 rounded-lg text-sm font-medium transition-colors
							{!locationDirty || savingLocation
								? 'bg-zinc-800 text-zinc-500 cursor-not-allowed'
								: 'bg-amber-500 text-zinc-950 hover:bg-amber-400'}"
					>
						{savingLocation ? 'Saving...' : 'Save location'}
					</button>
					{#if locationMessage}
						<span class="text-xs {locationMessage === 'Location saved.' ? 'text-green-400' : 'text-red-400'}">
							{locationMessage}
						</span>
					{/if}
				</div>
			</div>

			<!-- Existing mappings -->
			<div class="space-y-4 p-4 rounded-xl bg-zinc-900 border border-zinc-800">
				<div>
					<h2 class="text-lg font-semibold text-zinc-200">Current loop mapping</h2>
					<p class="text-xs text-zinc-500">These decoder IDs are already mapped on this track.</p>
				</div>

				{#if track.loops.length === 0}
					<p class="text-sm text-zinc-500">No timing loops mapped yet.</p>
				{:else}
					<div class="space-y-2">
						{#each track.loops as loop}
							<div class="flex items-center justify-between p-3 rounded-lg border border-zinc-800 bg-zinc-950">
								<div>
									<p class="text-sm text-zinc-200">{loop.name}</p>
									<p class="text-xs font-mono text-zinc-500">{loop.decoder_id}</p>
								</div>
								<span class="px-2 py-0.5 rounded text-xs font-medium {rolePill(loop.is_start ? 'start' : loop.is_finish ? 'finish' : 'split')}">
									{loop.is_start ? 'START' : loop.is_finish ? 'FINISH' : 'SPLIT'}
								</span>
							</div>
						{/each}
					</div>
				{/if}
			</div>
		</div>

		<!-- Discovery -->
		<div class="space-y-4 p-4 rounded-xl bg-zinc-900 border border-zinc-800">
			<div class="flex flex-wrap items-center gap-3 justify-between">
				<div>
					<h2 class="text-lg font-semibold text-zinc-200">Live decoder discovery (track-scoped)</h2>
					<p class="text-xs text-zinc-500">
						Shows decoders observed from ingest events for this track only.
					</p>
				</div>
				<div class="flex items-center gap-2">
					<label for="window" class="text-xs text-zinc-500">Window</label>
					<select
						id="window"
						value={windowSeconds}
						onchange={(e) => updateWindow((e.currentTarget as HTMLSelectElement).value)}
						class="px-2 py-1 bg-zinc-800 border border-zinc-700 rounded text-xs"
					>
						<option value={60}>60s</option>
						<option value={180}>3m</option>
						<option value={300}>5m</option>
						<option value={900}>15m</option>
					</select>
					<button
						type="button"
						onclick={() => setListening(!listening)}
						class="px-3 py-1 rounded text-xs font-medium transition-colors
							{listening ? 'bg-green-500/20 text-green-300' : 'bg-zinc-800 text-zinc-300'}"
					>
						{listening ? 'Listening' : 'Paused'}
					</button>
					<button
						type="button"
						onclick={() => refreshDiscovery()}
						disabled={discoveryLoading}
						class="px-3 py-1 rounded text-xs bg-zinc-800 text-zinc-300 hover:bg-zinc-700 disabled:opacity-50"
					>
						Refresh
					</button>
				</div>
			</div>

			{#if discoveryLoading && !discovery}
				<div class="text-sm text-zinc-500">Loading discovery data...</div>
			{:else if !discovery || discovery.decoders.length === 0}
				<div class="text-sm text-zinc-500">
					No decoders discovered for this track in the selected window. Start the track-client feed and
					click refresh.
				</div>
			{:else}
				<div class="text-xs text-zinc-500">
					Sampled {discovery.sampled_messages} messages in last {discovery.window_seconds}s.
				</div>

				{#if discovery.gate_beacons.length > 0}
					<div class="flex flex-wrap gap-2 text-xs">
						{#each discovery.gate_beacons as beacon}
							<span class="px-2 py-0.5 rounded bg-amber-500/15 text-amber-300 border border-amber-500/20">
								Gate beacon {beacon.transponder_id}: {beacon.hits} hits
							</span>
						{/each}
					</div>
				{/if}

				<div class="overflow-x-auto border border-zinc-800 rounded-lg">
					<table class="w-full text-sm">
						<thead class="bg-zinc-950 text-zinc-500">
							<tr>
								<th class="text-left px-3 py-2 font-medium">Decoder ID</th>
								<th class="text-left px-3 py-2 font-medium">Seen</th>
								<th class="text-left px-3 py-2 font-medium">Traffic</th>
								<th class="text-left px-3 py-2 font-medium">Mapped</th>
								<th class="text-left px-3 py-2 font-medium">Quick map</th>
							</tr>
						</thead>
						<tbody class="divide-y divide-zinc-800">
							{#each discovery.decoders as decoder}
								<tr class="bg-zinc-900/40">
									<td class="px-3 py-2 font-mono text-zinc-200">{decoder.decoder_id}</td>
									<td class="px-3 py-2 text-xs text-zinc-400">{formatUtcTimestamp(decoder.last_seen)}</td>
									<td class="px-3 py-2 text-xs text-zinc-400">
										P:{decoder.passing_count} &middot; S:{decoder.status_count} &middot; V:{decoder.version_count}
										{#if decoder.gate_hits > 0}
											<span class="ml-2 px-1.5 py-0.5 rounded bg-amber-500/20 text-amber-300">
												Gate hits: {decoder.gate_hits}
											</span>
										{/if}
									</td>
									<td class="px-3 py-2 text-xs">
										{#if decoder.mapped_loop_id}
											<span class="px-2 py-0.5 rounded {rolePill(decoder.mapped_role)}">
												{decoder.mapped_loop_name} ({decoder.mapped_role?.toUpperCase()})
											</span>
										{:else}
											<span class="text-zinc-500">Unmapped</span>
										{/if}
									</td>
									<td class="px-3 py-2">
										{#if decoder.mapped_loop_id}
											<span class="text-xs text-zinc-500">Already mapped</span>
										{:else}
											<div class="flex flex-wrap items-center gap-2">
												<input
													type="text"
													value={drafts[decoder.decoder_id]?.name ?? ''}
													oninput={(e) => updateDraftName(decoder.decoder_id, (e.currentTarget as HTMLInputElement).value)}
													class="min-w-[10rem] px-2 py-1 bg-zinc-800 border border-zinc-700 rounded text-xs"
												/>
												<select
													value={drafts[decoder.decoder_id]?.role ?? 'split'}
													onchange={(e) => updateDraftRole(decoder.decoder_id, (e.currentTarget as HTMLSelectElement).value as LoopRole)}
													class="px-2 py-1 bg-zinc-800 border border-zinc-700 rounded text-xs"
												>
													<option value="start">Start</option>
													<option value="split">Split</option>
													<option value="finish">Finish</option>
												</select>
												<button
													type="button"
													onclick={() => addLoopFromDecoder(decoder)}
													disabled={addingDecoderId === decoder.decoder_id}
													class="px-2.5 py-1 rounded text-xs font-medium
														{addingDecoderId === decoder.decoder_id
															? 'bg-zinc-800 text-zinc-500'
															: 'bg-amber-500 text-zinc-950 hover:bg-amber-400'}"
												>
													{addingDecoderId === decoder.decoder_id ? 'Adding...' : 'Map'}
												</button>
											</div>
										{/if}
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{/if}
		</div>
	{/if}
</div>
