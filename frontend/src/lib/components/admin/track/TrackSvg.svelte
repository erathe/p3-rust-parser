<script lang="ts">
	import type { TimingLoop } from '$lib/api/types';

	interface EditableSection {
		name: string;
		section_type: string;
		length_m: number;
		loop_id: string | null;
	}

	let {
		sections,
		loops,
		selectedIndex = null,
		onSelect
	}: {
		sections: EditableSection[];
		loops: TimingLoop[];
		selectedIndex: number | null;
		onSelect: (i: number | null) => void;
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

	const SVG_W = 700;
	const SVG_H = 400;
	const MARGIN_X = 40;
	const MARGIN_Y = 35;
	const TRACK_STROKE = 24;
	const SECTION_STROKE = 18;

	// Winding BMX track geometry
	// 4 rows connected by 3 U-turns (like the reference image)
	const NUM_ROWS = 4;
	const trackLeft = MARGIN_X;
	const trackRight = SVG_W - MARGIN_X;
	const trackTop = MARGIN_Y;
	const trackBottom = SVG_H - MARGIN_Y;

	// Row spacing is divided evenly; the U-turn radius = half the row spacing
	// so the semicircle exactly bridges two adjacent rows
	const rowSpacing = (trackBottom - trackTop) / (NUM_ROWS - 1);
	const TURN_RADIUS = rowSpacing / 2;
	const straightLen = trackRight - trackLeft - 2 * TURN_RADIUS;

	// Row Y positions
	function rowY(row: number): number {
		return trackTop + row * rowSpacing;
	}

	// Build the full path as segments with known lengths
	// Row 0: left→right, Turn right, Row 1: right→left, Turn left,
	// Row 2: left→right, Turn right, Row 3: right→left (finish)
	interface Segment {
		type: 'straight' | 'turn';
		length: number;
	}

	const turnArcLen = Math.PI * TURN_RADIUS;
	const segments: Segment[] = [];

	for (let row = 0; row < NUM_ROWS; row++) {
		segments.push({ type: 'straight', length: straightLen });
		if (row < NUM_ROWS - 1) {
			segments.push({ type: 'turn', length: turnArcLen });
		}
	}

	const totalPathLen = segments.reduce((sum, s) => sum + s.length, 0);

	/**
	 * Map t ∈ [0,1] to {x, y} on the winding snake path.
	 *
	 * Row 0: left → right  (top)
	 * U-turn right side     (curves down)
	 * Row 1: right → left
	 * U-turn left side      (curves down)
	 * Row 2: left → right
	 * U-turn right side     (curves down)
	 * Row 3: right → left   (bottom / finish)
	 */
	function pathPoint(t: number): { x: number; y: number } {
		let d = Math.max(0, Math.min(t, 1)) * totalPathLen;
		let row = 0;
		let segIdx = 0;

		for (const seg of segments) {
			if (d <= seg.length + 0.001) {
				if (seg.type === 'straight') {
					const frac = d / seg.length;
					const y = rowY(row);
					const goingRight = row % 2 === 0;

					if (goingRight) {
						const x = trackLeft + TURN_RADIUS + frac * straightLen;
						return { x, y };
					} else {
						const x = trackRight - TURN_RADIUS - frac * straightLen;
						return { x, y };
					}
				} else {
					// U-turn
					const frac = d / seg.length;
					const goingRight = row % 2 === 0;
					const yCenter = rowY(row) + rowSpacing / 2;

					if (goingRight) {
						// Turn is on the right side
						const cx = trackRight - TURN_RADIUS;
						const startAngle = -Math.PI / 2; // pointing up (from row going right)
						const angle = startAngle + frac * Math.PI;
						return {
							x: cx + Math.cos(angle) * TURN_RADIUS,
							y: yCenter + Math.sin(angle) * TURN_RADIUS
						};
					} else {
						// Turn is on the left side
						const cx = trackLeft + TURN_RADIUS;
						const startAngle = -Math.PI / 2; // pointing up
						const angle = startAngle - frac * Math.PI; // go the other way
						return {
							x: cx + Math.cos(angle) * TURN_RADIUS,
							y: yCenter + Math.sin(angle) * TURN_RADIUS
						};
					}
				}
			}
			d -= seg.length;
			if (seg.type === 'turn') {
				row++;
			}
			segIdx++;
		}

		// Fallback: end of path
		const lastRowY = rowY(NUM_ROWS - 1);
		const lastGoingRight = (NUM_ROWS - 1) % 2 === 0;
		return {
			x: lastGoingRight ? trackRight - TURN_RADIUS : trackLeft + TURN_RADIUS,
			y: lastRowY
		};
	}

	/**
	 * Get tangent angle at parameter t (for label rotation and tick marks)
	 */
	function tangentAngle(t: number): number {
		const dt = 0.0005;
		const t1 = Math.max(0, t - dt);
		const t2 = Math.min(1, t + dt);
		const p1 = pathPoint(t1);
		const p2 = pathPoint(t2);
		return Math.atan2(p2.y - p1.y, p2.x - p1.x);
	}

	// Build section geometry — distribute sections proportionally along the path
	let sectionData = $derived.by(() => {
		if (sections.length === 0) return [];

		const totalLength = sections.reduce((sum, s) => sum + s.length_m, 0);
		if (totalLength === 0) return [];

		const result: Array<{
			points: string;
			midpoint: { x: number; y: number };
			color: string;
			tStart: number;
			tEnd: number;
			hasDecoder: boolean;
			decoderPoint: { x: number; y: number };
		}> = [];

		let cumulative = 0;
		for (const section of sections) {
			const tStart = cumulative / totalLength;
			const tEnd = (cumulative + section.length_m) / totalLength;
			const tMid = (tStart + tEnd) / 2;

			// Sample points along the section's portion of the path
			const numSamples = Math.max(10, Math.ceil((tEnd - tStart) * 100));
			const pts: string[] = [];
			for (let j = 0; j <= numSamples; j++) {
				const t = tStart + (j / numSamples) * (tEnd - tStart);
				const p = pathPoint(t);
				pts.push(`${p.x.toFixed(1)},${p.y.toFixed(1)}`);
			}

			const mid = pathPoint(tMid);

			result.push({
				points: pts.join(' '),
				midpoint: mid,
				color: SECTION_COLORS[section.section_type] ?? SECTION_COLORS.custom,
				tStart,
				tEnd,
				hasDecoder: section.loop_id !== null,
				decoderPoint: pathPoint(tMid)
			});

			cumulative += section.length_m;
		}

		return result;
	});

	// Full track outline path for background
	let outlinePath = $derived.by(() => {
		const pts: string[] = [];
		const steps = 200;
		for (let i = 0; i <= steps; i++) {
			const p = pathPoint(i / steps);
			pts.push(`${p.x.toFixed(1)},${p.y.toFixed(1)}`);
		}
		return pts.join(' ');
	});

	// Direction arrow near the start
	let arrowData = $derived.by(() => {
		const p = pathPoint(0.02);
		const angle = tangentAngle(0.02);
		return { x: p.x, y: p.y, angle: (angle * 180) / Math.PI };
	});

	// Gate/Start marker position
	let gatePos = $derived(pathPoint(0));

	// Finish marker position
	let finishPos = $derived(pathPoint(1));

	// Push label to the side that has more room (above or below the track line)
	function labelOffset(t: number): { x: number; y: number } {
		const angle = tangentAngle(t);
		const perpX = -Math.sin(angle);
		const perpY = Math.cos(angle);
		// Push below the track path (positive y = downward in SVG)
		const offset = 24;
		return { x: perpX * offset, y: perpY * offset };
	}
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
	class="rounded-xl bg-zinc-950 border border-zinc-800 overflow-hidden"
	onclick={() => onSelect(null)}
>
	<svg viewBox="0 0 {SVG_W} {SVG_H}" class="w-full">
		<!-- Track background outline -->
		<polyline
			points={outlinePath}
			fill="none"
			stroke="#27272a"
			stroke-width={TRACK_STROKE}
			stroke-linejoin="round"
			stroke-linecap="round"
		/>

		{#if sectionData.length > 0}
			<!-- Section segments -->
			{#each sectionData as sd, i}
				<polyline
					points={sd.points}
					fill="none"
					stroke={sd.color}
					stroke-width={SECTION_STROKE}
					stroke-linecap="butt"
					stroke-linejoin="round"
					opacity={selectedIndex === null ? 0.7 : selectedIndex === i ? 1 : 0.3}
					class="cursor-pointer transition-opacity duration-150"
					onclick={(e) => { e.stopPropagation(); onSelect(i); }}
				/>
			{/each}

			<!-- Section boundary tick marks -->
			{#each sectionData as sd, i}
				{@const p = pathPoint(sd.tStart)}
				{@const angle = tangentAngle(sd.tStart)}
				{@const perpX = -Math.sin(angle)}
				{@const perpY = Math.cos(angle)}
				{#if i > 0}
					<line
						x1={p.x - perpX * 14}
						y1={p.y - perpY * 14}
						x2={p.x + perpX * 14}
						y2={p.y + perpY * 14}
						stroke="#52525b"
						stroke-width="1.5"
					/>
				{/if}
			{/each}

			<!-- Decoder markers -->
			{#each sectionData as sd, i}
				{#if sd.hasDecoder}
					<circle
						cx={sd.decoderPoint.x}
						cy={sd.decoderPoint.y}
						r="5"
						fill="#f59e0b"
						stroke="#000"
						stroke-width="1.5"
					/>
					<!-- Small antenna line -->
					<line
						x1={sd.decoderPoint.x}
						y1={sd.decoderPoint.y - 7}
						x2={sd.decoderPoint.x}
						y2={sd.decoderPoint.y - 12}
						stroke="#f59e0b"
						stroke-width="1.5"
						stroke-linecap="round"
					/>
				{/if}
			{/each}

			<!-- Section labels -->
			{#each sectionData as sd, i}
				{@const tMid = (sd.tStart + sd.tEnd) / 2}
				{@const off = labelOffset(tMid)}
				<text
					x={sd.midpoint.x + off.x}
					y={sd.midpoint.y + off.y}
					text-anchor="middle"
					dominant-baseline="middle"
					fill={selectedIndex === i ? '#fff' : '#a1a1aa'}
					font-size="9"
					font-weight={selectedIndex === i ? '600' : '400'}
					class="pointer-events-none select-none"
				>
					{sections[i].name}
				</text>
			{/each}

			<!-- Length label for selected section -->
			{#if selectedIndex !== null && sectionData[selectedIndex]}
				{@const sd = sectionData[selectedIndex]}
				{@const tMid = (sd.tStart + sd.tEnd) / 2}
				{@const off = labelOffset(tMid)}
				<text
					x={sd.midpoint.x + off.x}
					y={sd.midpoint.y + off.y + 12}
					text-anchor="middle"
					dominant-baseline="middle"
					fill="#71717a"
					font-size="8"
					font-family="monospace"
					class="pointer-events-none select-none"
				>
					{sections[selectedIndex].length_m.toFixed(1)}m
				</text>
			{/if}
		{:else}
			<!-- Empty state -->
			<text
				x={SVG_W / 2}
				y={SVG_H / 2}
				text-anchor="middle"
				fill="#52525b"
				font-size="14"
				class="select-none"
			>
				No sections configured
			</text>
		{/if}

		<!-- START marker -->
		{#if sections.length > 0}
			<g transform="translate({gatePos.x}, {gatePos.y})">
				<rect x="-22" y="-24" width="44" height="16" rx="3" fill="#f59e0b" opacity="0.9" />
				<text
					x="0"
					y="-15"
					text-anchor="middle"
					dominant-baseline="middle"
					fill="#000"
					font-size="8"
					font-weight="700"
					class="select-none"
				>
					START
				</text>
			</g>
		{/if}

		<!-- FINISH marker -->
		{#if sections.length > 0}
			<g transform="translate({finishPos.x}, {finishPos.y})">
				<rect x="-22" y="8" width="44" height="16" rx="3" fill="#f97316" opacity="0.9" />
				<text
					x="0"
					y="17"
					text-anchor="middle"
					dominant-baseline="middle"
					fill="#000"
					font-size="8"
					font-weight="700"
					class="select-none"
				>
					FINISH
				</text>
			</g>
		{/if}

		<!-- Direction arrow near start -->
		{#if sections.length > 0}
			<g transform="translate({arrowData.x}, {arrowData.y}) rotate({arrowData.angle})">
				<polygon points="0,-4 8,0 0,4" fill="#71717a" />
			</g>
		{/if}
	</svg>
</div>
