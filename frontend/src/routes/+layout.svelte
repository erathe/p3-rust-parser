<script lang="ts">
	import '../app.css';

	let { children } = $props();
</script>

<div class="min-h-screen bg-zinc-950 text-zinc-100">
	<nav class="border-b border-zinc-800 bg-zinc-900/80 backdrop-blur-sm sticky top-0 z-50">
		<div class="max-w-7xl mx-auto px-4 h-14 flex items-center gap-6">
			<a href="/" class="text-lg font-bold tracking-tight text-white">
				<span class="text-amber-400">P3</span> BMX Timing
			</a>

			<div class="flex gap-1 text-sm">
				<a
					href="/admin/tracks"
					class="px-3 py-1.5 rounded-md hover:bg-zinc-800 text-zinc-400 hover:text-zinc-100 transition-colors"
				>
					Tracks
				</a>
				<a
					href="/admin/riders"
					class="px-3 py-1.5 rounded-md hover:bg-zinc-800 text-zinc-400 hover:text-zinc-100 transition-colors"
				>
					Riders
				</a>
				<a
					href="/admin/events"
					class="px-3 py-1.5 rounded-md hover:bg-zinc-800 text-zinc-400 hover:text-zinc-100 transition-colors"
				>
					Events
				</a>
				<a
					href="/admin/decoder"
					class="px-3 py-1.5 rounded-md hover:bg-zinc-800 text-zinc-400 hover:text-zinc-100 transition-colors"
				>
					Decoder
				</a>
			</div>

			<div class="ml-auto flex gap-1 text-sm">
				<a
					href="/race/control"
					class="px-3 py-1.5 rounded-md bg-amber-500/10 text-amber-400 hover:bg-amber-500/20 transition-colors"
				>
					Race Control
				</a>
				<a
					href="/race/display"
					class="px-3 py-1.5 rounded-md bg-zinc-800 text-zinc-300 hover:bg-zinc-700 transition-colors"
				>
					Display
				</a>
			</div>
		</div>
	</nav>

	<main class="max-w-7xl mx-auto px-4 py-6">
		{@render children()}
	</main>
</div>
