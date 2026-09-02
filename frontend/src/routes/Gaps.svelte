<script lang="ts">
  import { onMount } from 'svelte';
  import type { GapReport } from '../lib/types';
  import { Flame, TrendingUp, Eye, ExternalLink, Sparkles, Tag, ArrowRight, Layers, Check } from 'lucide-svelte';
  import MediaCard from '../components/MediaCard.svelte';
  import VideoAnalyticsModal from '../components/VideoAnalyticsModal.svelte';

  let report = $state<GapReport | null>(null);
  let tagGaps = $state<any[]>([]);
  let selectedVideoId = $state<string | null>(null);
  let loading = $state(true);

  async function loadGaps() {
    loading = true;
    try {
      const res = await fetch('/api/gaps');
      if (res.ok) {
        report = await res.json();
      }

      const tagsRes = await fetch('/api/tags/gaps');
      if (tagsRes.ok) {
        tagGaps = await tagsRes.json();
      }
    } catch {
      // ignore
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    loadGaps();
  });
</script>

<div class="space-y-8">

  <!-- Header -->
  <div class="bg-gradient-to-r from-gray-900 via-amber-950/20 to-gray-900 border border-gray-800 p-6 rounded-2xl">
    <div class="flex items-center space-x-2">
      <Flame class="w-6 h-6 text-amber-400" />
      <h2 class="text-xl font-extrabold text-white tracking-tight">
        Competitor Outlier & Content Gap Radar
      </h2>
    </div>
    <p class="text-xs text-gray-400 mt-1 max-w-2xl">
      Identifies competitor breakout videos outperforming their baseline channel average by 2.0x to 10x+, alongside semantic tag opportunities with zero channel saturation.
    </p>
  </div>

  {#if loading}
    <div class="p-16 text-center text-gray-400 bg-gray-900/30 rounded-2xl border border-gray-800">
      <div class="inline-block animate-spin rounded-full h-8 w-8 border-t-2 border-b-2 border-amber-500 mb-3"></div>
      <p class="text-xs font-medium">Scanning competitor corpus for breakout multipliers and BM25 clusters...</p>
    </div>
  {:else}
    
    <!-- 1. Breakout Outlier Videos Grid -->
    <div class="space-y-4">
      <div class="flex items-center justify-between">
        <div class="flex items-center space-x-2">
          <TrendingUp class="w-4 h-4 text-amber-400" />
          <h3 class="text-sm font-bold text-gray-200 uppercase tracking-wider">
            Breakout Outlier Videos ({report?.outliers?.length || 0})
          </h3>
        </div>
        <span class="text-xs text-gray-500 font-mono">
          Threshold: &ge; 2.0x Channel Mean Views
        </span>
      </div>

      {#if !report || (!report.outliers?.length)}
        <div class="p-8 text-center text-xs text-gray-500 bg-gray-900/30 rounded-2xl border border-gray-800">
          No outlier videos detected yet. Ingest competitor channels to populate the breakout radar.
        </div>
      {:else}
        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-5">
          {#each report.outliers as outlier}
            {@const videoItem = {
              video_id: outlier.video_id,
              title: outlier.title,
              channel: outlier.channel_name || outlier.channel,
              views: outlier.views,
              outlier_multiplier: outlier.multiple,
              thumb_url: `https://i.ytimg.com/vi/${outlier.video_id}/hqdefault.jpg`,
            }}
            <MediaCard 
              video={videoItem}
              onSelect={() => selectedVideoId = outlier.video_id}
            />
          {/each}
        </div>
      {/if}
    </div>

    <!-- 2. High-Opportunity Semantic Tag Gaps -->
    {#if tagGaps && tagGaps.length > 0}
      <div class="space-y-4 pt-4 border-t border-gray-800/80">
        <div class="flex items-center space-x-2">
          <Tag class="w-4 h-4 text-indigo-400" />
          <h3 class="text-sm font-bold text-gray-200 uppercase tracking-wider">
            High-Opportunity Competitor Tag Gaps ({tagGaps.length})
          </h3>
        </div>

        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3.5">
          {#each tagGaps.slice(0, 12) as tagGap}
            <div class="p-4 rounded-xl bg-gray-900/60 border border-gray-800 flex items-center justify-between">
              <div class="space-y-1">
                <span class="text-xs font-bold text-gray-200 font-mono">
                  #{tagGap.tag || tagGap.name}
                </span>
                <p class="text-[11px] text-gray-400">
                  {tagGap.competitor_usage || 0} competitor videos · {tagGap.your_usage || 0} on own channel
                </p>
              </div>

              <div class="text-right">
                <span class="text-xs font-mono font-extrabold text-emerald-400">
                  +{((tagGap.opportunity_score || 0) * 100).toFixed(0)} opp
                </span>
              </div>
            </div>
          {/each}
        </div>
      </div>
    {/if}

  {/if}

  <!-- Video Analytics Modal -->
  {#if selectedVideoId}
    <VideoAnalyticsModal 
      videoId={selectedVideoId} 
      onClose={() => selectedVideoId = null}
    />
  {/if}

</div>
