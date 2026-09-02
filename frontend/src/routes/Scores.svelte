<script lang="ts">
  import { onMount } from 'svelte';
  import type { ScoreRow } from '../lib/types';
  import { Sparkles, LayoutGrid, List, Search, TrendingUp, SlidersHorizontal } from 'lucide-svelte';
  import MediaCard from '../components/MediaCard.svelte';
  import VideoAnalyticsModal from '../components/VideoAnalyticsModal.svelte';

  let scores = $state<ScoreRow[]>([]);
  let loading = $state(true);
  let searchQuery = $state('');
  let viewMode = $state<'grid' | 'table'>('grid');
  let sortBy = $state<'score' | 'views' | 'outlier'>('score');
  let selectedVideoId = $state<string | null>(null);

  let { onNavigate }: { onNavigate?: (route: string) => void } = $props();

  async function loadScores() {
    loading = true;
    try {
      const res = await fetch('/api/scores');
      if (res.ok) {
        scores = await res.json();
      }
    } catch {
      // ignore
    } finally {
      loading = false;
    }
  }

  const sortedAndFilteredScores = $derived(
    scores
      .filter(s => 
        s.title.toLowerCase().includes(searchQuery.toLowerCase()) ||
        s.channel_name.toLowerCase().includes(searchQuery.toLowerCase())
      )
      .sort((a, b) => {
        if (sortBy === 'views') {
          return (b.views || 0) - (a.views || 0);
        }
        if (sortBy === 'outlier') {
          return (b.outlier_multiplier || 1) - (a.outlier_multiplier || 1);
        }
        return (b.overall_score || 0) - (a.overall_score || 0);
      })
  );

  onMount(() => {
    loadScores();
  });
</script>

<div class="space-y-6">

  <!-- Header & Toolbar -->
  <div class="flex flex-col md:flex-row md:items-center justify-between gap-4 bg-gray-900/60 border border-gray-800 p-5 rounded-2xl">
    <div>
      <div class="flex items-center space-x-2">
        <Sparkles class="w-5 h-5 text-indigo-400" />
        <h2 class="text-xl font-extrabold text-white tracking-tight">
          Video Intelligence & Quality Ratings
        </h2>
      </div>
      <p class="text-xs text-gray-400 mt-1">
        {scores.length} indexed YouTube videos with 18 SEO and 7 GEO algorithmic signals, breakout outlier multipliers, and metadata.
      </p>
    </div>

    <!-- Controls Row -->
    <div class="flex flex-wrap items-center gap-2.5">
      
      <!-- Search Box -->
      <div class="relative">
        <Search class="w-3.5 h-3.5 text-gray-500 absolute left-3 top-1/2 -translate-y-1/2" />
        <input 
          type="text" 
          bind:value={searchQuery}
          placeholder="Filter videos or channels..."
          class="pl-8 pr-3.5 py-2 rounded-xl bg-gray-950 border border-gray-800 text-gray-100 text-xs focus:outline-none focus:border-indigo-500 w-56"
        />
      </div>

      <!-- Sort Dropdown -->
      <select
        bind:value={sortBy}
        class="px-3 py-2 rounded-xl bg-gray-950 border border-gray-800 text-gray-200 text-xs focus:outline-none focus:border-indigo-500 cursor-pointer"
      >
        <option value="score">Sort by Quality Score</option>
        <option value="views">Sort by Total Views</option>
        <option value="outlier">Sort by Breakout Multiplier</option>
      </select>

      <!-- View Switcher (Grid / Table) -->
      <div class="flex items-center bg-gray-950 p-1 rounded-xl border border-gray-800">
        <button
          onclick={() => viewMode = 'grid'}
          class="p-1.5 rounded-lg text-xs transition-colors cursor-pointer {viewMode === 'grid' ? 'bg-indigo-600 text-white font-bold' : 'text-gray-400 hover:text-white'}"
          title="YouTube Cards Grid View"
        >
          <LayoutGrid class="w-4 h-4" />
        </button>
        <button
          onclick={() => viewMode = 'table'}
          class="p-1.5 rounded-lg text-xs transition-colors cursor-pointer {viewMode === 'table' ? 'bg-indigo-600 text-white font-bold' : 'text-gray-400 hover:text-white'}"
          title="Dense Table View"
        >
          <List class="w-4 h-4" />
        </button>
      </div>

    </div>
  </div>

  {#if loading}
    <div class="p-16 text-center text-gray-400 bg-gray-900/30 rounded-2xl border border-gray-800">
      <div class="inline-block animate-spin rounded-full h-8 w-8 border-t-2 border-b-2 border-indigo-500 mb-3"></div>
      <p class="text-xs font-medium">Loading YouTube video cards from storage...</p>
    </div>
  {:else if sortedAndFilteredScores.length === 0}
    <div class="p-16 text-center text-gray-400 bg-gray-900/30 rounded-2xl border border-gray-800">
      <p class="text-sm font-medium text-gray-300">No matching videos found</p>
      <p class="text-xs text-gray-500 mt-1">Try adjusting your search query or filter criteria.</p>
    </div>
  {:else if viewMode === 'grid'}
    
    <!-- YouTube Cards Grid View -->
    <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-5">
      {#each sortedAndFilteredScores as video}
        <MediaCard 
          {video}
          onSelect={() => selectedVideoId = video.video_id}
          onInspectGaps={() => onNavigate?.('gaps')}
        />
      {/each}
    </div>

  {:else}

    <!-- Dense Table View -->
    <div class="rounded-2xl bg-gray-900/50 border border-gray-800 overflow-hidden">
      <table class="w-full text-left text-xs">
        <thead class="bg-gray-950/80 border-b border-gray-800 text-gray-400 font-mono uppercase text-[10px]">
          <tr>
            <th class="p-4">Thumbnail & Video Title</th>
            <th class="p-4">Channel</th>
            <th class="p-4 text-right">Breakout</th>
            <th class="p-4 text-right">Overall Score</th>
            <th class="p-4 text-right">Freshness</th>
            <th class="p-4 text-right">Authority</th>
            <th class="p-4 text-right">Views</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-gray-800/60 text-gray-300">
          {#each sortedAndFilteredScores as row}
            <tr 
              class="hover:bg-gray-800/30 transition-colors cursor-pointer"
              onclick={() => selectedVideoId = row.video_id}
            >
              <td class="p-4 font-medium text-gray-100 max-w-md">
                <div class="flex items-center space-x-3">
                  <img 
                    src={row.thumb_url || `https://i.ytimg.com/vi/${row.video_id}/hqdefault.jpg`}
                    alt={row.title}
                    class="w-16 h-9 rounded-md object-cover bg-gray-950 border border-gray-800 shrink-0"
                    loading="lazy"
                  />
                  <span class="truncate block hover:text-indigo-400" title={row.title}>{row.title}</span>
                </div>
              </td>
              <td class="p-4 text-gray-400">
                {row.channel_name}
              </td>
              <td class="p-4 text-right font-mono font-bold {row.outlier_multiplier && row.outlier_multiplier >= 2.0 ? 'text-amber-400' : 'text-gray-500'}">
                {row.outlier_multiplier ? `${row.outlier_multiplier}x` : '1.0x'}
              </td>
              <td class="p-4 text-right font-mono font-bold text-indigo-400">
                {row.overall_score?.toFixed(1) || '0.0'}
              </td>
              <td class="p-4 text-right font-mono text-gray-400">
                {row.freshness_score?.toFixed(1) || '0.0'}
              </td>
              <td class="p-4 text-right font-mono text-gray-400">
                {row.authority_score?.toFixed(1) || '0.0'}
              </td>
              <td class="p-4 text-right font-mono text-gray-300">
                {row.views?.toLocaleString() || 0}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

  {/if}

  <!-- Video Analytics Modal -->
  {#if selectedVideoId}
    <VideoAnalyticsModal 
      videoId={selectedVideoId} 
      onClose={() => selectedVideoId = null}
    />
  {/if}

</div>

