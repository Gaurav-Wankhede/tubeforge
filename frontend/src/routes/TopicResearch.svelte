<script lang="ts">
  import { onMount } from 'svelte';
  import { rpc } from '../lib/rpc.svelte';
  import { 
    Search, 
    Sparkles, 
    Kanban, 
    Check, 
    Clock, 
    TrendingUp, 
    Flame, 
    FileText,
    ExternalLink,
    Cpu,
    Radio,
    Tag,
    Copy,
    BarChart3,
    AlertCircle,
    Layers,
    Eye,
    Award,
    Smartphone,
    ShieldCheck,
    CheckCircle2,
    Sliders
  } from 'lucide-svelte';
  import VideoAnalyticsModal from '../components/VideoAnalyticsModal.svelte';

  interface RankingVideo {
    position: number;
    title: string;
    channel: string;
    views: number;
    seo_score: number;
    video_id?: string;
  }

  interface SuggestedTag {
    tag: string;
    usage: number;
  }

  interface RelatedKeyword {
    keyword: string;
    popularity_rank: number;
  }

  interface TitleVariation {
    archetype: string;
    title: string;
    mobile_preview_45: string;
    rationale: string;
  }

  interface TopicAnalysisResponse {
    topic: string;
    volume: string;
    verdict: string;
    scores: {
      opportunity: number;
      competition: number;
      keyword_score: number;
    };
    demand: {
      actively_published: boolean;
      avg_views_per_ranking_video: number;
      serp_total: number;
    };
    gap: {
      demand_views: number;
      score: number;
      supply_videos: number;
      type: string;
    };
    packaging: {
      title: string;
      title_variations?: TitleVariation[];
      mobile_preview_45?: string;
      description: string;
      tags: string[];
      has_colon?: boolean;
      char_count?: number;
    };
    ranking_chart: RankingVideo[];
    related_keywords: RelatedKeyword[];
    suggested_tags: SuggestedTag[];
  }

  let query = $state('');
  let serpLimit = $state(8);
  let loading = $state(false);
  let research = $state<TopicAnalysisResponse | null>(null);
  let selectedTitle = $state<string>('');
  let createdTicketMessage = $state<string | null>(null);
  let activeTab = $state<'packaging' | 'variations' | 'serp' | 'keywords'>('packaging');
  let selectedVideoId = $state<string | null>(null);
  let copiedText = $state<string | null>(null);

  let { 
    initialQuery = '', 
    onNavigate 
  }: { 
    initialQuery?: string; 
    onNavigate?: (route: string, param?: string) => void 
  } = $props();

  onMount(() => {
    if (initialQuery && initialQuery.trim()) {
      query = initialQuery;
      handleSearch();
    } else {
      query = 'Rust Async Tokio Web Development';
      handleSearch();
    }
  });

  async function handleSearch() {
    if (!query.trim()) return;
    loading = true;
    createdTicketMessage = null;
    try {
      const res = await fetch(`/api/analysis/topic?q=${encodeURIComponent(query.trim())}&serp=${serpLimit}`);
      if (res.ok) {
        research = await res.json();
        if (research?.packaging?.title) {
          selectedTitle = research.packaging.title;
        }
      }
    } catch {
      // ignore
    } finally {
      loading = false;
    }
  }

  async function createKanbanTicket(titleToUse?: string) {
    if (!research) return;
    const finalTitle = titleToUse || selectedTitle || research.packaging.title;
    try {
      const res = await rpc.call('kanban.from-research', {
        topic: finalTitle,
        channel: 'TECHVERSE',
        framework: 'Core Mental Model',
        optimal_duration_sec: 720,
      });
      if (res && res.message) {
        createdTicketMessage = res.message;
      }
    } catch (e: any) {
      createdTicketMessage = `Error creating ticket — ${e.message}`;
    }
  }

  function copyToClipboard(text: string) {
    navigator.clipboard.writeText(text);
    copiedText = text;
    setTimeout(() => copiedText = null, 1500);
  }
</script>

<div class="space-y-6">

  <!-- Search Header -->
  <div class="p-6 rounded-2xl bg-gray-900/60 border border-gray-800 space-y-4">
    <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-2">
      <div>
        <h2 class="text-xl font-extrabold text-white tracking-tight flex items-center space-x-2">
          <span>Topic Intelligence & Empirical Packaging Engine</span>
        </h2>
        <p class="text-xs text-gray-400">
          Zero-colon architecture, 45-character mobile viewport optimization, and 4 empirical title archetypes.
        </p>
      </div>
    </div>

    <!-- Search Bar -->
    <form onsubmit={(e) => { e.preventDefault(); handleSearch(); }} class="flex flex-col sm:flex-row gap-2">
      <div class="relative flex-1">
        <Search class="w-4 h-4 text-gray-400 absolute left-3.5 top-1/2 -translate-y-1/2" />
        <input 
          type="text" 
          bind:value={query}
          placeholder="Enter developer topic (e.g. Linux Kernel Syscalls, SQLite Internals, Rust Async)..."
          class="w-full pl-10 pr-4 py-2.5 rounded-xl bg-gray-950 border border-gray-800 text-gray-100 placeholder-gray-500 text-sm focus:outline-none focus:border-indigo-500/80 transition-colors"
        />
      </div>

      <button
        type="submit"
        disabled={loading}
        class="px-5 py-2.5 rounded-xl bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 text-white text-xs font-bold shadow-lg shadow-indigo-500/20 transition-all flex items-center justify-center space-x-2 cursor-pointer"
      >
        {#if loading}
          <span class="animate-spin w-4 h-4 border-2 border-white border-t-transparent rounded-full"></span>
          <span>Analyzing SERP...</span>
        {:else}
          <Sparkles class="w-4 h-4" />
          <span>Analyze & Package</span>
        {/if}
      </button>
    </form>
  </div>

  {#if research}
    
    <!-- Topic Overview & Action Banner -->
    <div class="p-5 rounded-2xl bg-gray-900/60 border border-gray-800 space-y-4">
      <div class="flex flex-col md:flex-row md:items-center justify-between gap-4">
        
        <div class="space-y-1">
          <div class="flex items-center space-x-2.5">
            <span class="text-lg font-extrabold text-white">
              "{research.topic}"
            </span>
            <span class="px-2.5 py-0.5 rounded-full text-xs font-mono font-bold bg-indigo-500/10 text-indigo-400 border border-indigo-500/20">
              {research.volume} Volume
            </span>
            <span class="px-2 py-0.5 rounded-full text-[10px] font-mono font-bold bg-emerald-500/10 text-emerald-400 border border-emerald-500/20 flex items-center space-x-1">
              <CheckCircle2 class="w-3 h-3" />
              <span>Zero Colons Verified</span>
            </span>
          </div>
          <p class="text-xs text-gray-400 max-w-3xl">
            {research.verdict}
          </p>
        </div>

        <!-- Quick Action Buttons -->
        <div class="flex items-center space-x-2 shrink-0">
          <button
            type="button"
            onclick={() => createKanbanTicket()}
            class="inline-flex items-center space-x-1.5 px-3.5 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-bold shadow-lg shadow-indigo-500/20 transition-all cursor-pointer"
          >
            <Kanban class="w-3.5 h-3.5" />
            <span>Send Selected to Kanban</span>
          </button>

          {#if onNavigate}
            <button
              type="button"
              onclick={() => onNavigate('teleprompter')}
              class="inline-flex items-center space-x-1.5 px-3.5 py-2 rounded-xl bg-gray-800 hover:bg-gray-750 text-gray-200 text-xs font-medium transition-all cursor-pointer"
            >
              <Radio class="w-3.5 h-3.5 text-purple-400" />
              <span>Open in Teleprompter</span>
            </button>
          {/if}
        </div>

      </div>

      {#if createdTicketMessage}
        <div class="p-3 rounded-xl bg-emerald-500/10 border border-emerald-500/30 text-emerald-300 text-xs flex items-center justify-between">
          <span>{createdTicketMessage}</span>
          {#if onNavigate}
            <button 
              type="button"
              onclick={() => onNavigate('kanban')}
              class="underline font-semibold hover:text-white cursor-pointer"
            >
              View on Kanban Board →
            </button>
          {/if}
        </div>
      {/if}

      <!-- Metric Score Cards Grid -->
      <div class="grid grid-cols-2 sm:grid-cols-4 gap-3 pt-2">
        
        <div class="p-3.5 rounded-xl bg-gray-950/80 border border-gray-800 space-y-1">
          <span class="text-[10px] font-mono text-gray-400 uppercase tracking-wider block">Opportunity Score</span>
          <div class="text-xl font-extrabold text-emerald-400 font-mono">
            {research.scores.opportunity.toFixed(1)} <span class="text-xs text-gray-500 font-normal">/ 100</span>
          </div>
          <span class="text-[10px] text-gray-500 block">Demand vs Competition Ratio</span>
        </div>

        <div class="p-3.5 rounded-xl bg-gray-950/80 border border-gray-800 space-y-1">
          <span class="text-[10px] font-mono text-gray-400 uppercase tracking-wider block">Competition Score</span>
          <div class="text-xl font-extrabold text-amber-400 font-mono">
            {research.scores.competition.toFixed(1)} <span class="text-xs text-gray-500 font-normal">/ 100</span>
          </div>
          <span class="text-[10px] text-gray-500 block">SERP Saturation Barrier</span>
        </div>

        <div class="p-3.5 rounded-xl bg-gray-950/80 border border-gray-800 space-y-1">
          <span class="text-[10px] font-mono text-gray-400 uppercase tracking-wider block">Avg Ranking Views</span>
          <div class="text-xl font-extrabold text-purple-400 font-mono">
            {Math.round(research.demand.avg_views_per_ranking_video).toLocaleString()}
          </div>
          <span class="text-[10px] text-gray-500 block">Average views of Top SERP</span>
        </div>

        <div class="p-3.5 rounded-xl bg-gray-950/80 border border-gray-800 space-y-1">
          <span class="text-[10px] font-mono text-gray-400 uppercase tracking-wider block">Market Gap Signal</span>
          <div class="text-xs font-bold text-indigo-300 font-mono truncate">
            {research.gap.type}
          </div>
          <span class="text-[10px] text-gray-500 block">Strategy Recommendation</span>
        </div>

      </div>

    </div>

    <!-- Navigation Tabs -->
    <div class="flex space-x-2 border-b border-gray-800 pb-1">
      <button
        type="button"
        onclick={() => activeTab = 'packaging'}
        class="px-4 py-2 rounded-xl text-xs font-bold transition-all cursor-pointer {
          activeTab === 'packaging' 
            ? 'bg-indigo-600 text-white shadow-md shadow-indigo-500/10' 
            : 'text-gray-400 hover:text-gray-200 hover:bg-gray-850'
        }"
      >
        Live Packaging & Mobile Viewport
      </button>

      <button
        type="button"
        onclick={() => activeTab = 'variations'}
        class="px-4 py-2 rounded-xl text-xs font-bold transition-all cursor-pointer {
          activeTab === 'variations' 
            ? 'bg-indigo-600 text-white shadow-md shadow-indigo-500/10' 
            : 'text-gray-400 hover:text-gray-200 hover:bg-gray-850'
        }"
      >
        4 Research Archetypes ({research.packaging?.title_variations?.length || 4})
      </button>

      <button
        type="button"
        onclick={() => activeTab = 'serp'}
        class="px-4 py-2 rounded-xl text-xs font-bold transition-all cursor-pointer {
          activeTab === 'serp' 
            ? 'bg-indigo-600 text-white shadow-md shadow-indigo-500/10' 
            : 'text-gray-400 hover:text-gray-200 hover:bg-gray-850'
        }"
      >
        SERP Competitor Rankings ({research.ranking_chart.length})
      </button>

      <button
        type="button"
        onclick={() => activeTab = 'keywords'}
        class="px-4 py-2 rounded-xl text-xs font-bold transition-all cursor-pointer {
          activeTab === 'keywords' 
            ? 'bg-indigo-600 text-white shadow-md shadow-indigo-500/10' 
            : 'text-gray-400 hover:text-gray-200 hover:bg-gray-850'
        }"
      >
        Tags & Related Keywords ({research.suggested_tags.length})
      </button>
    </div>

    <!-- Tab 1: Live Packaging & Mobile Viewport Simulator -->
    {#if activeTab === 'packaging'}
      <div class="space-y-5">
        
        <!-- Live Mobile Viewport Simulator Card -->
        <div class="p-5 rounded-2xl bg-gradient-to-r from-gray-900 via-gray-900/90 to-gray-950 border border-gray-800 shadow-xl space-y-4">
          <div class="flex items-center justify-between">
            <div class="flex items-center space-x-2">
              <Smartphone class="w-4 h-4 text-indigo-400" />
              <h3 class="text-xs font-bold text-white uppercase tracking-wider">
                Mobile Viewport Simulation (First 45 Characters Truncation)
              </h3>
            </div>
            <span class="text-[10px] font-mono text-gray-400">
              {selectedTitle.length} characters
            </span>
          </div>

          <!-- Mobile App Feed Mock Card -->
          <div class="p-4 rounded-xl bg-gray-950 border border-gray-800 max-w-lg space-y-3 shadow-inner">
            <div class="flex items-center space-x-2 text-[10px] font-mono text-gray-500">
              <span class="w-2 h-2 rounded-full bg-emerald-500"></span>
              <span>YouTube Mobile Feed Appearance</span>
            </div>
            
            <div class="space-y-1">
              <!-- Visual highlight of the first 45 chars -->
              <p class="text-sm font-bold text-white leading-snug">
                <span class="text-indigo-300 underline decoration-indigo-500/50 decoration-2 underline-offset-4">
                  {selectedTitle.slice(0, 45)}
                </span>
                <span class="text-gray-500">
                  {selectedTitle.length > 45 ? selectedTitle.slice(45) : ''}
                </span>
              </p>
              <span class="text-[10px] text-gray-500 block pt-1">
                Underlined portion is guaranteed visible on mobile screens before feed truncation.
              </span>
            </div>
          </div>
        </div>

        <!-- High-CTR Title Suggestion -->
        <div class="p-5 rounded-2xl bg-gray-900/60 border border-gray-800 space-y-3">
          <div class="flex items-center justify-between">
            <span class="text-xs font-bold text-indigo-400 uppercase tracking-wider flex items-center space-x-1.5">
              <Sparkles class="w-3.5 h-3.5" />
              <span>Active Packaging Title (Zero-Colon Rule Enforced)</span>
            </span>
            <div class="flex items-center space-x-2">
              <button 
                type="button" 
                onclick={() => copyToClipboard(selectedTitle)}
                class="text-xs text-gray-400 hover:text-white flex items-center space-x-1 cursor-pointer"
              >
                <Copy class="w-3.5 h-3.5" />
                <span>{copiedText === selectedTitle ? 'Copied!' : 'Copy Title'}</span>
              </button>
            </div>
          </div>

          <div class="flex items-center space-x-2">
            <input 
              type="text" 
              bind:value={selectedTitle}
              class="w-full text-base font-extrabold text-white font-mono bg-gray-950/80 p-3.5 rounded-xl border border-gray-800 focus:outline-none focus:border-indigo-500"
            />
          </div>
        </div>

        <!-- Recommended Tags Cloud -->
        {#if research.packaging.tags && research.packaging.tags.length > 0}
          <div class="p-5 rounded-2xl bg-gray-900/60 border border-gray-800 space-y-3">
            <div class="flex items-center justify-between">
              <span class="text-xs font-bold text-emerald-400 uppercase tracking-wider flex items-center space-x-1.5">
                <Tag class="w-3.5 h-3.5" />
                <span>Optimized Tag Set ({research.packaging.tags.length} Tags)</span>
              </span>
              <button 
                type="button" 
                onclick={() => copyToClipboard(research.packaging.tags.join(', '))}
                class="text-xs text-gray-400 hover:text-white flex items-center space-x-1 cursor-pointer"
              >
                <Copy class="w-3.5 h-3.5" />
                <span>Copy All</span>
              </button>
            </div>
            <div class="flex flex-wrap gap-2">
              {#each research.packaging.tags as tag}
                <button
                  type="button"
                  onclick={() => copyToClipboard(tag)}
                  class="px-3 py-1.5 rounded-xl text-xs bg-gray-950/80 border border-gray-800 hover:border-indigo-500 text-gray-300 hover:text-white transition-all cursor-pointer"
                >
                  {tag}
                </button>
              {/each}
            </div>
          </div>
        {/if}

        <!-- Description Blueprint -->
        <div class="p-5 rounded-2xl bg-gray-900/60 border border-gray-800 space-y-3">
          <div class="flex items-center justify-between">
            <span class="text-xs font-bold text-purple-400 uppercase tracking-wider flex items-center space-x-1.5">
              <FileText class="w-3.5 h-3.5" />
              <span>Description Blueprint (StoryBrand Problem ➔ Architecture Guide)</span>
            </span>
            <button 
              type="button" 
              onclick={() => copyToClipboard(research.packaging.description)}
              class="text-xs text-gray-400 hover:text-white flex items-center space-x-1 cursor-pointer"
            >
              <Copy class="w-3.5 h-3.5" />
              <span>Copy Description</span>
            </button>
          </div>
          <pre class="text-xs text-gray-300 font-mono bg-gray-950/80 p-4 rounded-xl border border-gray-800 whitespace-pre-wrap leading-relaxed">
{research.packaging.description}
          </pre>
        </div>

      </div>
    {/if}

    <!-- Tab 2: 4 Empirical Title Archetypes -->
    {#if activeTab === 'variations'}
      <div class="space-y-4">
        <div class="p-4 rounded-xl bg-gray-950/60 border border-gray-800 text-xs text-gray-400">
          Pick from 4 research-backed packaging formulas tested for maximum CTR and cognitive retention.
        </div>

        {#if research.packaging.title_variations && research.packaging.title_variations.length > 0}
          <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
            {#each research.packaging.title_variations as varItem}
              <div class="p-5 rounded-2xl bg-gray-950/80 border {selectedTitle === varItem.title ? 'border-indigo-500 shadow-lg shadow-indigo-500/10' : 'border-gray-800'} space-y-3 flex flex-col justify-between">
                
                <div class="space-y-2">
                  <div class="flex items-center justify-between">
                    <span class="px-2.5 py-0.5 rounded-full text-[10px] font-mono font-bold bg-indigo-500/10 text-indigo-400 border border-indigo-500/20">
                      {varItem.archetype}
                    </span>
                    {#if selectedTitle === varItem.title}
                      <span class="text-[10px] font-mono text-emerald-400 font-bold flex items-center space-x-1">
                        <Check class="w-3 h-3" />
                        <span>ACTIVE SELECTION</span>
                      </span>
                    {/if}
                  </div>

                  <h4 class="text-sm font-extrabold text-white font-mono leading-snug">
                    {varItem.title}
                  </h4>

                  <p class="text-xs text-gray-400 leading-relaxed">
                    {varItem.rationale}
                  </p>
                </div>

                <div class="pt-3 border-t border-gray-900 flex items-center justify-between">
                  <span class="text-[10px] font-mono text-gray-500">
                    Mobile: <span class="text-gray-300 font-semibold">{varItem.mobile_preview_45}</span>
                  </span>

                  <div class="flex items-center space-x-2">
                    <button
                      type="button"
                      onclick={() => selectedTitle = varItem.title}
                      class="px-3 py-1.5 rounded-lg text-xs font-bold transition-all cursor-pointer {
                        selectedTitle === varItem.title 
                          ? 'bg-emerald-600 text-white' 
                          : 'bg-gray-800 hover:bg-gray-700 text-gray-200'
                      }"
                    >
                      {selectedTitle === varItem.title ? 'Selected' : 'Use Title'}
                    </button>
                  </div>
                </div>

              </div>
            {/each}
          </div>
        {/if}
      </div>
    {/if}

    <!-- Tab 3: SERP Competitor Rankings Grid -->
    {#if activeTab === 'serp'}
      <div class="space-y-4">
        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
          {#each research.ranking_chart as item}
            <div class="p-4 rounded-xl bg-gray-950/80 border border-gray-800 space-y-3 flex flex-col justify-between">
              
              <div class="space-y-2">
                <div class="flex items-center justify-between text-xs font-mono">
                  <span class="px-2 py-0.5 rounded-md bg-indigo-500/20 text-indigo-300 font-bold">
                    #{item.position} SERP
                  </span>
                  <span class="text-emerald-400 font-bold">
                    {item.views.toLocaleString()} views
                  </span>
                </div>

                <h4 class="text-sm font-bold text-gray-100 line-clamp-2">
                  {item.title}
                </h4>
              </div>

              <div class="flex items-center justify-between pt-2 border-t border-gray-900 text-xs font-mono text-gray-400">
                <span class="truncate max-w-[180px]">{item.channel}</span>
                <span class="px-2 py-0.5 rounded bg-gray-900 text-gray-400 text-[10px]">
                  SEO: {item.seo_score.toFixed(1)}
                </span>
              </div>

            </div>
          {/each}
        </div>
      </div>
    {/if}

    <!-- Tab 4: Tags & Related Keywords -->
    {#if activeTab === 'keywords'}
      <div class="space-y-5">
        
        <!-- Suggested Tags with Frequency -->
        <div class="p-5 rounded-2xl bg-gray-900/60 border border-gray-800 space-y-3">
          <h3 class="text-xs font-bold text-emerald-400 uppercase tracking-wider flex items-center space-x-2">
            <Tag class="w-3.5 h-3.5" />
            <span>High-Frequency Competitor Tags</span>
          </h3>
          <div class="flex flex-wrap gap-2 pt-1">
            {#each research.suggested_tags as item}
              <button
                type="button"
                onclick={() => copyToClipboard(item.tag)}
                class="px-3 py-1.5 rounded-xl text-xs bg-gray-950/80 border border-gray-800 hover:border-emerald-500 text-gray-300 hover:text-emerald-300 transition-all flex items-center space-x-1.5 cursor-pointer group"
              >
                <span>{item.tag}</span>
                <span class="text-[10px] font-mono text-gray-500 group-hover:text-emerald-400">
                  ({item.usage}x)
                </span>
              </button>
            {/each}
          </div>
        </div>

        <!-- Related Keywords from Autocomplete -->
        {#if research.related_keywords && research.related_keywords.length > 0}
          <div class="p-5 rounded-2xl bg-gray-900/60 border border-gray-800 space-y-3">
            <h3 class="text-xs font-bold text-purple-400 uppercase tracking-wider flex items-center space-x-2">
              <TrendingUp class="w-3.5 h-3.5" />
              <span>Related Search Queries (Autocomplete Order)</span>
            </h3>
            <div class="grid grid-cols-1 sm:grid-cols-2 gap-2 pt-1">
              {#each research.related_keywords as item}
                <div class="p-3 rounded-xl bg-gray-950/80 border border-gray-800 flex items-center justify-between text-xs">
                  <span class="text-gray-200 font-mono">{item.keyword}</span>
                  <span class="px-2 py-0.5 rounded bg-purple-500/10 text-purple-400 font-mono text-[10px]">
                    Rank #{item.popularity_rank}
                  </span>
                </div>
              {/each}
            </div>
          </div>
        {/if}

      </div>
    {/if}

  {/if}

</div>
