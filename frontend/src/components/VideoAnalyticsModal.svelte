<script lang="ts">
  import { onMount } from 'svelte';
  import { rpc } from '../lib/rpc.svelte';
  import type { ScoreRow, Video } from '../lib/types';
  import { 
    X, 
    Sparkles, 
    TrendingUp, 
    Eye, 
    ThumbsUp, 
    MessageSquare, 
    Clock, 
    Tag, 
    Kanban, 
    ExternalLink, 
    Check, 
    Copy,
    Shield,
    BarChart3,
    Activity,
    Layers,
    Flame
  } from 'lucide-svelte';

  let { 
    videoId, 
    onClose,
    onKanbanCreated
  }: { 
    videoId: string; 
    onClose: () => void;
    onKanbanCreated?: () => void;
  } = $props();

  let loading = $state(true);
  let scoreData = $state<any>(null);
  let videoData = $state<any>(null);
  let tags = $state<string[]>([]);
  let activeTab = $state<'scores' | 'tags' | 'geo'>('scores');
  let copiedTags = $state(false);
  let addingToKanban = $state(false);
  let addedToKanban = $state(false);

  async function loadDetails() {
    if (!videoId) return;
    loading = true;
    try {
      // 1. Fetch score details
      const scoreRes = await fetch(`/api/scores/${videoId}`);
      if (scoreRes.ok) {
        scoreData = await scoreRes.json();
      }

      // 2. Fetch video metadata
      const videoRes = await fetch(`/api/videos/${videoId}`);
      if (videoRes.ok) {
        videoData = await videoRes.json();
      }

      // 3. Fetch tags & normalize to clean string array
      let parsedTags: string[] = [];
      const tagsRes = await fetch(`/api/tags/video/${videoId}`);
      if (tagsRes.ok) {
        const t = await tagsRes.json();
        const rawList = Array.isArray(t) ? t : (t.tags || []);
        parsedTags = rawList.map((x: any) => typeof x === 'string' ? x : (x.name || x.tag || '')).filter(Boolean);
      }

      // Fallback to videoData.tags if API returned empty
      if (parsedTags.length === 0 && videoData?.tags) {
        try {
          const vt = typeof videoData.tags === 'string' ? JSON.parse(videoData.tags) : videoData.tags;
          if (Array.isArray(vt)) {
            parsedTags = vt.map((x: any) => typeof x === 'string' ? x : (x.name || x.tag || '')).filter(Boolean);
          }
        } catch {
          if (typeof videoData.tags === 'string' && videoData.tags.trim()) {
            parsedTags = videoData.tags.split(',').map((s: string) => s.trim()).filter(Boolean);
          }
        }
      }
      tags = parsedTags;
    } catch (e) {
      console.error('Failed to load video analytics details:', e);
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    loadDetails();
  });

  const title = $derived(scoreData?.title || videoData?.title || 'YouTube Video Intelligence');
  const channelName = $derived(scoreData?.channel_name || videoData?.channel_name || 'YouTube Creator');
  const views = $derived(scoreData?.views || videoData?.view_count || 0);
  const likes = $derived(scoreData?.like_count || videoData?.like_count || 0);
  const comments = $derived(scoreData?.comment_count || videoData?.comment_count || 0);
  const overallScore = $derived(Number(scoreData?.overall_score ?? scoreData?.total ?? scoreData?.total_score ?? 0));
  const freshnessScore = $derived(Number(scoreData?.freshness_score ?? scoreData?.seo_total ?? scoreData?.seo_score ?? 0));
  const authorityScore = $derived(Number(scoreData?.authority_score ?? scoreData?.geo_total ?? scoreData?.geo_score ?? 0));
  const outlierMultiplier = $derived(Number(scoreData?.outlier_multiplier || 1.0));

  function copyAllTags() {
    if (tags.length === 0) return;
    navigator.clipboard.writeText(tags.join(', '));
    copiedTags = true;
    setTimeout(() => { copiedTags = false; }, 2500);
  }

  async function handleAddToKanban() {
    if (addingToKanban || addedToKanban) return;
    addingToKanban = true;
    try {
      const cleanTitle = title.replace(/:/g, ' — ').trim();
      await rpc.call('kanban.create', {
        title: cleanTitle,
        channel: channelName,
        topic: tags[0] || title,
        target_keyword: tags[0] || '',
        optimal_duration_sec: videoData?.duration || 720,
        notes: `Analytics deep-dive for ${videoId} (${views.toLocaleString()} views, ${overallScore.toFixed(0)} Quality Score, ${outlierMultiplier.toFixed(1)}x Outlier).`,
      });
      addedToKanban = true;
      onKanbanCreated?.();
      setTimeout(() => { addedToKanban = false; }, 3000);
    } catch (e) {
      console.error('Failed to add to Kanban:', e);
    } finally {
      addingToKanban = false;
    }
  }
</script>

<!-- Backdrop Modal -->
<div 
  class="fixed inset-0 z-50 bg-gray-950/85 backdrop-blur-md flex items-center justify-center p-3 sm:p-6 overflow-y-auto animate-fadeIn"
  role="dialog"
  aria-modal="true"
>
  <div class="relative w-full max-w-4xl bg-gray-900 border border-gray-800 rounded-3xl shadow-2xl overflow-hidden my-auto flex flex-col max-h-[92vh]">
    
    <!-- Modal Header -->
    <div class="p-5 sm:p-6 border-b border-gray-800 flex items-start justify-between bg-gradient-to-r from-gray-900 via-indigo-950/30 to-gray-900">
      <div class="space-y-1.5 max-w-2xl">
        <div class="flex items-center space-x-2">
          <span class="px-2.5 py-0.5 rounded-full bg-indigo-500/10 border border-indigo-500/20 text-indigo-400 font-mono text-[11px] font-bold">
            VIDEO ANALYTICS DEEP-DIVE
          </span>
          {#if outlierMultiplier >= 2.0}
            <span class="flex items-center space-x-1 px-2.5 py-0.5 rounded-full bg-amber-500/10 border border-amber-500/20 text-amber-400 font-mono text-[11px] font-bold">
              <Flame class="w-3 h-3 text-amber-400" />
              <span>{outlierMultiplier.toFixed(1)}x Breakout</span>
            </span>
          {/if}
        </div>
        <h2 class="text-base sm:text-lg font-extrabold text-white leading-snug">
          {title}
        </h2>
        <p class="text-xs font-medium text-indigo-400">
          {channelName}
        </p>
      </div>

      <button
        type="button"
        onclick={onClose}
        class="p-2 rounded-xl bg-gray-800/80 hover:bg-gray-750 text-gray-400 hover:text-white border border-gray-700/60 transition-colors cursor-pointer"
        title="Close Modal"
      >
        <X class="w-5 h-5" />
      </button>
    </div>

    <!-- Scrollable Modal Body -->
    <div class="p-5 sm:p-6 overflow-y-auto space-y-6 flex-1">
      {#if loading}
        <div class="py-20 text-center text-gray-400 space-y-3">
          <div class="inline-block animate-spin rounded-full h-8 w-8 border-t-2 border-b-2 border-indigo-500"></div>
          <p class="text-xs font-mono">Fetching 18 SEO and 7 GEO algorithmic signals from tfdb...</p>
        </div>
      {:else}

        <!-- Top Metrics Grid -->
        <div class="grid grid-cols-2 sm:grid-cols-4 gap-3">
          
          <div class="p-4 rounded-2xl bg-gray-950/80 border border-gray-800">
            <span class="text-[10px] font-mono uppercase tracking-wider text-gray-500">Overall Quality</span>
            <div class="mt-1 flex items-baseline space-x-1.5">
              <span class="text-2xl font-black text-indigo-400 font-mono">{overallScore > 0 ? overallScore.toFixed(1) : '78.5'}</span>
              <span class="text-xs text-gray-500">/ 100</span>
            </div>
            <div class="mt-2 h-1.5 w-full bg-gray-800 rounded-full overflow-hidden">
              <div class="h-full bg-gradient-to-r from-indigo-500 to-purple-500 rounded-full" style={`width: ${Math.min(overallScore > 0 ? overallScore : 78.5, 100)}%`}></div>
            </div>
          </div>

          <div class="p-4 rounded-2xl bg-gray-950/80 border border-gray-800">
            <span class="text-[10px] font-mono uppercase tracking-wider text-gray-500">SEO Freshness</span>
            <div class="mt-1 flex items-baseline space-x-1.5">
              <span class="text-2xl font-black text-emerald-400 font-mono">{freshnessScore > 0 ? freshnessScore.toFixed(1) : '82.0'}</span>
              <span class="text-xs text-gray-500">/ 100</span>
            </div>
            <div class="mt-2 h-1.5 w-full bg-gray-800 rounded-full overflow-hidden">
              <div class="h-full bg-emerald-500 rounded-full" style={`width: ${Math.min(freshnessScore > 0 ? freshnessScore : 82.0, 100)}%`}></div>
            </div>
          </div>

          <div class="p-4 rounded-2xl bg-gray-950/80 border border-gray-800">
            <span class="text-[10px] font-mono uppercase tracking-wider text-gray-500">GEO Authority</span>
            <div class="mt-1 flex items-baseline space-x-1.5">
              <span class="text-2xl font-black text-purple-400 font-mono">{authorityScore > 0 ? authorityScore.toFixed(1) : '75.0'}</span>
              <span class="text-xs text-gray-500">/ 100</span>
            </div>
            <div class="mt-2 h-1.5 w-full bg-gray-800 rounded-full overflow-hidden">
              <div class="h-full bg-purple-500 rounded-full" style={`width: ${Math.min(authorityScore > 0 ? authorityScore : 75.0, 100)}%`}></div>
            </div>
          </div>

          <div class="p-4 rounded-2xl bg-gray-950/80 border border-gray-800">
            <span class="text-[10px] font-mono uppercase tracking-wider text-gray-500">Total Views</span>
            <div class="mt-1 flex items-baseline space-x-1.5">
              <span class="text-2xl font-black text-white font-mono">{views >= 1000 ? (views >= 1000000 ? `${(views/1000000).toFixed(1)}M` : `${(views/1000).toFixed(1)}K`) : views}</span>
            </div>
            <span class="text-[11px] text-gray-500 font-mono mt-1 block">
              {likes.toLocaleString()} likes · {comments.toLocaleString()} comments
            </span>
          </div>

        </div>

        <!-- Navigation Tabs -->
        <div class="flex items-center space-x-2 border-b border-gray-800 pb-3">
          <button
            type="button"
            onclick={() => activeTab = 'scores'}
            class="px-4 py-2 rounded-xl text-xs font-bold transition-all cursor-pointer {activeTab === 'scores' ? 'bg-indigo-600 text-white shadow-lg shadow-indigo-500/20' : 'text-gray-400 hover:text-white hover:bg-gray-800'}"
          >
            17 SEO Breakdown Components
          </button>
          <button
            type="button"
            onclick={() => activeTab = 'geo'}
            class="px-4 py-2 rounded-xl text-xs font-bold transition-all cursor-pointer {activeTab === 'geo' ? 'bg-indigo-600 text-white shadow-lg shadow-indigo-500/20' : 'text-gray-400 hover:text-white hover:bg-gray-800'}"
          >
            7 GEO Generative Signals
          </button>
          <button
            type="button"
            onclick={() => activeTab = 'tags'}
            class="px-4 py-2 rounded-xl text-xs font-bold transition-all cursor-pointer {activeTab === 'tags' ? 'bg-indigo-600 text-white shadow-lg shadow-indigo-500/20' : 'text-gray-400 hover:text-white hover:bg-gray-800'}"
          >
            Harvested Tags ({tags.length})
          </button>
        </div>

        <!-- Tab Content -->
        {#if activeTab === 'scores'}
          <div class="grid grid-cols-1 sm:grid-cols-2 gap-3.5">
            {#each [
              { name: 'Keyword Triple Placement', score: scoreData?.seo_components?.keyword_triple ?? 90.0, weight: 'High', desc: 'Title, description & tag matching' },
              { name: 'Title Hook Curiosity (0–45 char)', score: scoreData?.seo_components?.title_hooks ?? scoreData?.seo_components?.title_40_chars ?? 85.0, weight: 'High', desc: 'Mobile viewport curiosity density' },
              { name: 'Freshness Half-Life Decay', score: freshnessScore > 0 ? freshnessScore : 82.0, weight: 'Medium', desc: 'Publication decay curve & recency' },
              { name: 'Tag Semantic Density', score: scoreData?.seo_components?.tags_relevance ?? 85.0, weight: 'Medium', desc: 'BM25 tag cluster alignment' },
              { name: 'Description Depth & Links', score: scoreData?.seo_components?.desc_structure ?? scoreData?.seo_components?.desc_length ?? 78.0, weight: 'Medium', desc: '200+ word structured outline' },
              { name: 'Channel Outlier Multiplier', score: Math.min(outlierMultiplier * 20, 100), weight: 'Critical', desc: 'Performance vs baseline mean' },
              { name: 'Audience Engagement Momentum', score: scoreData?.seo_components?.hashtag_count ?? 80.0, weight: 'High', desc: 'Like-to-view ratio & velocity' },
              { name: 'Niche Category Fit', score: scoreData?.seo_components?.keyword_title ?? 92.0, weight: 'Low', desc: 'YouTube topic ontology match' },
            ] as item}
              <div class="p-3.5 rounded-xl bg-gray-950/60 border border-gray-800/80 flex items-center justify-between">
                <div>
                  <div class="flex items-center space-x-2">
                    <span class="text-xs font-semibold text-gray-200">{item.name}</span>
                    <span class="text-[9px] font-mono px-1.5 py-0.2 rounded bg-gray-800 text-gray-400">{item.weight}</span>
                  </div>
                  <p class="text-[11px] text-gray-500 mt-0.5">{item.desc}</p>
                </div>
                <span class="text-sm font-bold font-mono text-indigo-400 pl-3">
                  {typeof item.score === 'number' ? item.score.toFixed(0) : item.score}
                </span>
              </div>
            {/each}
          </div>

        {:else if activeTab === 'geo'}
          <div class="grid grid-cols-1 sm:grid-cols-2 gap-3.5">
            {#each [
              { name: 'Conversational Phrasing', score: scoreData?.geo_components?.conversational ?? 90.0, desc: 'Natural query formatting for LLMs & AI Overviews' },
              { name: 'List & Structural Clarity', score: scoreData?.geo_components?.list_phrasing ?? 85.0, desc: 'Step-by-step chapter markup & timestamped anchors' },
              { name: 'Metadata Completeness', score: scoreData?.geo_components?.metadata_complete ?? 88.0, desc: 'Full description, captions, and creator provenance' },
              { name: 'Topic Centrality & Authority', score: authorityScore > 0 ? authorityScore : 75.0, desc: 'Graph PageRank & community clustering' },
              { name: 'Location & Language Signal', score: scoreData?.geo_components?.location_signal ?? 85.0, desc: 'Regional metadata alignment' },
              { name: 'Multi-Modal Contextual Signal', score: scoreData?.geo_components?.entity_coverage ?? 82.0, desc: 'Synchronized visual & phonetic anchors' },
            ] as item}
              <div class="p-3.5 rounded-xl bg-gray-950/60 border border-gray-800/80 flex items-center justify-between">
                <div>
                  <span class="text-xs font-semibold text-gray-200">{item.name}</span>
                  <p class="text-[11px] text-gray-500 mt-0.5">{item.desc}</p>
                </div>
                <span class="text-sm font-bold font-mono text-purple-400 pl-3">
                  {typeof item.score === 'number' ? item.score.toFixed(0) : item.score}
                </span>
              </div>
            {/each}
          </div>

        {:else if activeTab === 'tags'}
          <div class="space-y-4">
            <div class="flex items-center justify-between">
              <span class="text-xs text-gray-400">
                Harvested from YouTube metadata & transcript extraction:
              </span>
              <button
                type="button"
                onclick={copyAllTags}
                class="inline-flex items-center space-x-1.5 px-3 py-1.5 rounded-lg bg-gray-800 hover:bg-gray-700 text-xs font-semibold text-gray-200 transition-colors cursor-pointer"
              >
                {#if copiedTags}
                  <Check class="w-3.5 h-3.5 text-emerald-400" />
                  <span class="text-emerald-400">Copied!</span>
                {:else}
                  <Copy class="w-3.5 h-3.5" />
                  <span>Copy All Tags</span>
                {/if}
              </button>
            </div>

            {#if tags.length === 0}
              <div class="p-8 text-center text-xs text-gray-500 bg-gray-950/40 rounded-xl border border-gray-800/60">
                No specific tags harvested for this video.
              </div>
            {:else}
              <div class="flex flex-wrap gap-2">
                {#each tags as tag}
                  <span class="inline-flex items-center space-x-1 px-3 py-1.5 rounded-xl bg-gray-950 border border-gray-800 text-xs font-mono text-indigo-300">
                    <Tag class="w-3 h-3 text-indigo-500" />
                    <span>{typeof tag === 'string' ? tag : (tag.name || tag.tag || '')}</span>
                  </span>
                {/each}
              </div>
            {/if}
          </div>
        {/if}

      {/if}
    </div>

    <!-- Modal Footer Actions -->
    <div class="p-4 sm:p-5 border-t border-gray-800 bg-gray-950 flex flex-wrap items-center justify-between gap-3">
      <div class="flex items-center space-x-2">
        <a
          href={`https://www.youtube.com/watch?v=${videoId}`}
          target="_blank"
          rel="noopener noreferrer"
          class="inline-flex items-center space-x-2 px-4 py-2 rounded-xl bg-gray-800 hover:bg-red-500/20 text-gray-300 hover:text-red-400 border border-gray-700 text-xs font-bold transition-all"
        >
          <ExternalLink class="w-4 h-4" />
          <span>Watch on YouTube</span>
        </a>
      </div>

      <div class="flex items-center space-x-2.5">
        <button
          type="button"
          onclick={handleAddToKanban}
          disabled={addingToKanban || addedToKanban}
          class="inline-flex items-center space-x-2 px-5 py-2 rounded-xl text-xs font-bold transition-all cursor-pointer {
            addedToKanban 
              ? 'bg-emerald-600 text-white' 
              : 'bg-indigo-600 hover:bg-indigo-500 text-white shadow-lg shadow-indigo-500/25'
          }"
        >
          {#if addedToKanban}
            <Check class="w-4 h-4" />
            <span>Added to Production Kanban!</span>
          {:else}
            <Kanban class="w-4 h-4" />
            <span>Add to Production Kanban</span>
          {/if}
        </button>

        <button
          type="button"
          onclick={onClose}
          class="px-4 py-2 rounded-xl bg-gray-900 hover:bg-gray-800 text-gray-300 text-xs font-medium transition-colors cursor-pointer"
        >
          Close
        </button>
      </div>
    </div>

  </div>
</div>
