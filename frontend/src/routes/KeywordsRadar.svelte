<script lang="ts">
  import { onMount } from 'svelte';
  import { 
    KeyRound, 
    TrendingUp, 
    Flame, 
    Search, 
    ExternalLink, 
    Sparkles, 
    Tag, 
    ArrowUpRight,
    Kanban
  } from 'lucide-svelte';

  let keywords = $state<any[]>([]);
  let trending = $state<any[]>([]);
  let loading = $state(true);
  let searchQuery = $state('');

  let { onNavigate }: { onNavigate: (route: string, param?: string) => void } = $props();

  async function loadData() {
    loading = true;
    try {
      const kwRes = await fetch('/api/keywords');
      if (kwRes.ok) {
        keywords = await kwRes.json();
      }

      const trendRes = await fetch('/api/keywords/trending');
      if (trendRes.ok) {
        const t = await trendRes.json();
        trending = t.trending || [];
      }
    } catch (e) {
      console.error('Failed to load keywords:', e);
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    loadData();
  });

  const filteredKeywords = $derived(
    keywords.filter(k => !searchQuery || k.keyword.toLowerCase().includes(searchQuery.toLowerCase()))
  );
</script>

<div class="space-y-6">

  <!-- Header -->
  <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4 bg-gradient-to-r from-gray-900 via-indigo-950/30 to-gray-900 border border-gray-800 p-6 rounded-3xl">
    <div class="space-y-1">
      <div class="flex items-center space-x-2">
        <KeyRound class="w-5 h-5 text-indigo-400" />
        <h2 class="text-xl font-extrabold text-white tracking-tight">
          Keyword Opportunity & Search Radar
        </h2>
      </div>
      <p class="text-xs text-gray-400">
        Rank tracking, BM25 corpus resonance, and live competitor search trends across YouTube SERPs.
      </p>
    </div>

    <!-- Search input -->
    <div class="relative w-full sm:w-64">
      <Search class="absolute left-3 top-2.5 w-4 h-4 text-gray-500" />
      <input
        type="text"
        placeholder="Filter keywords..."
        bind:value={searchQuery}
        class="w-full pl-9 pr-3 py-2 rounded-xl bg-gray-950 border border-gray-800 text-xs text-gray-200 placeholder-gray-500 focus:outline-none focus:border-indigo-500"
      />
    </div>
  </div>

  <!-- Live Trending Keywords Section -->
  {#if trending.length > 0}
    <div class="space-y-3">
      <div class="flex items-center space-x-2 text-xs font-bold text-gray-300 uppercase tracking-wider">
        <Flame class="w-4 h-4 text-amber-400" />
        <span>Live Trending Search Opportunities ({trending.length})</span>
      </div>

      <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {#each trending as t}
          <div class="p-4 rounded-2xl bg-gray-900/70 border border-gray-800/80 hover:border-indigo-500/40 transition-all flex flex-col justify-between space-y-3">
            <div>
              <div class="flex items-center justify-between">
                <span class="text-[10px] font-mono font-bold px-2 py-0.5 rounded bg-indigo-500/10 text-indigo-400 border border-indigo-500/20">
                  {t.volume_label} Volume
                </span>
                <span class="text-xs font-mono font-bold text-emerald-400">
                  Opp: {t.score?.toFixed(1)} / 100
                </span>
              </div>
              <h3 class="text-sm font-bold text-white mt-2 leading-snug">
                {t.keyword}
              </h3>
              <p class="text-[11px] text-gray-500 mt-1 font-mono">
                SERP Mean Views: {t.serp_mean_views ? Math.round(t.serp_mean_views).toLocaleString() : '—'}
              </p>
            </div>

            <div class="flex items-center space-x-2 pt-2 border-t border-gray-800/60">
              <button
                type="button"
                onclick={() => onNavigate('research', t.keyword)}
                class="flex-1 inline-flex items-center justify-center space-x-1.5 px-3 py-1.5 rounded-lg bg-indigo-600/20 hover:bg-indigo-600/30 text-indigo-300 border border-indigo-500/30 text-xs font-semibold cursor-pointer transition-all"
              >
                <Search class="w-3.5 h-3.5" />
                <span>Deep SERP Research</span>
              </button>
            </div>
          </div>
        {/each}
      </div>
    </div>
  {/if}

  <!-- Tracked Keywords Table -->
  <div class="space-y-3">
    <div class="flex items-center space-x-2 text-xs font-bold text-gray-300 uppercase tracking-wider">
      <TrendingUp class="w-4 h-4 text-indigo-400" />
      <span>Tracked Corpus Keywords ({filteredKeywords.length})</span>
    </div>

    {#if loading}
      <div class="py-12 text-center text-gray-400 text-xs font-mono">
        Loading keyword rank positions...
      </div>
    {:else if filteredKeywords.length === 0}
      <div class="py-12 text-center text-gray-500 text-xs">
        No tracked keywords found matching "{searchQuery}".
      </div>
    {:else}
      <div class="overflow-x-auto rounded-2xl border border-gray-800 bg-gray-900/60">
        <table class="w-full text-left text-xs text-gray-300">
          <thead class="bg-gray-950/80 text-[11px] uppercase tracking-wider text-gray-400 font-mono border-b border-gray-800">
            <tr>
              <th class="px-5 py-3.5">Target Keyword</th>
              <th class="px-5 py-3.5 text-center">Rank Position</th>
              <th class="px-5 py-3.5 text-center">Trend Velocity</th>
              <th class="px-5 py-3.5 text-right">Actions</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-gray-800/60">
            {#each filteredKeywords as k}
              <tr class="hover:bg-gray-800/30 transition-colors">
                <td class="px-5 py-3.5 font-medium text-white">
                  <div class="flex items-center space-x-2">
                    <Tag class="w-3.5 h-3.5 text-indigo-400 shrink-0" />
                    <span>{k.keyword}</span>
                  </div>
                </td>
                <td class="px-5 py-3.5 text-center font-mono font-bold">
                  {#if k.rank !== null && k.rank !== undefined}
                    <span class="px-2.5 py-0.5 rounded-full bg-indigo-500/10 border border-indigo-500/20 text-indigo-400 text-xs">
                      #{k.rank}
                    </span>
                  {:else}
                    <span class="text-gray-600">—</span>
                  {/if}
                </td>
                <td class="px-5 py-3.5 text-center font-mono">
                  {#if k.trend !== null && k.trend !== undefined}
                    <span class={k.trend > 0 ? 'text-emerald-400' : (k.trend < 0 ? 'text-rose-400' : 'text-gray-400')}>
                      {k.trend > 0 ? `+${k.trend}` : k.trend}
                    </span>
                  {:else}
                    <span class="text-gray-600">—</span>
                  {/if}
                </td>
                <td class="px-5 py-3.5 text-right">
                  <button
                    type="button"
                    onclick={() => onNavigate('research', k.keyword)}
                    class="inline-flex items-center space-x-1 px-3 py-1 rounded-lg bg-gray-800 hover:bg-gray-700 text-gray-200 text-[11px] font-semibold transition-colors cursor-pointer"
                  >
                    <span>Analyze</span>
                    <ArrowUpRight class="w-3.5 h-3.5" />
                  </button>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </div>

</div>
