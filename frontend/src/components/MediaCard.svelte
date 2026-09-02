<script lang="ts">
  import type { SerpResult, ScoreRow } from '../lib/types';
  import { 
    Eye, 
    ThumbsUp, 
    ExternalLink, 
    Sparkles, 
    TrendingUp, 
    Clock, 
    Kanban, 
    Check, 
    Search,
    Play,
    BarChart3
  } from 'lucide-svelte';
  import { rpc } from '../lib/rpc.svelte';

  let { 
    video, 
    channelMean = 5000,
    onSelect,
    onInspectGaps,
    onKanbanCreated
  }: { 
    video: SerpResult | ScoreRow | any; 
    channelMean?: number;
    onSelect?: (video: any) => void;
    onInspectGaps?: (video: any) => void;
    onKanbanCreated?: () => void;
  } = $props();

  let addingToKanban = $state(false);
  let addedToKanban = $state(false);
  let thumbFallbackIndex = $state(0);

  const videoId = $derived(video.video_id || '');
  const title = $derived(video.title || 'Untitled Video');
  const channelName = $derived(video.channel || video.channel_name || 'YouTube Creator');
  const views = $derived(video.views || video.view_count || 0);
  const likes = $derived(video.like_count || 0);
  const durationSec = $derived(video.duration_sec || 0);
  const seoScore = $derived(video.seo_score || video.freshness_score || null);
  const geoScore = $derived(video.geo_score || video.authority_score || null);
  const overallScore = $derived(video.overall_score || null);

  const outlierMultiplier = $derived(
    video.outlier_multiplier !== undefined 
      ? video.outlier_multiplier.toFixed(1)
      : channelMean > 0 ? (views / channelMean).toFixed(1) : '1.0'
  );
  const isOutlier = $derived(parseFloat(outlierMultiplier) >= 2.0);

  const formattedViews = $derived(
    views >= 1_000_000 
      ? `${(views / 1_000_000).toFixed(1)}M` 
      : views >= 1_000 
      ? `${(views / 1_000).toFixed(1)}K` 
      : `${views}`
  );

  const formattedLikes = $derived(
    likes >= 1_000_000 
      ? `${(likes / 1_000_000).toFixed(1)}M` 
      : likes >= 1_000 
      ? `${(likes / 1_000).toFixed(1)}K` 
      : `${likes}`
  );

  function formatDuration(sec: number): string {
    if (!sec || sec <= 0) return '';
    const m = Math.floor(sec / 60);
    const s = Math.floor(sec % 60);
    const h = Math.floor(m / 60);
    if (h > 0) {
      const remM = m % 60;
      return `${h}:${remM.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`;
    }
    return `${m}:${s.toString().padStart(2, '0')}`;
  }

  function formatRelativeDate(dateStr: string | null | undefined): string {
    if (!dateStr) return '';
    try {
      const d = new Date(dateStr);
      if (isNaN(d.getTime())) return dateStr;
      const now = new Date();
      const diffDays = Math.floor((now.getTime() - d.getTime()) / (1000 * 60 * 60 * 24));
      if (diffDays < 1) return 'Today';
      if (diffDays === 1) return 'Yesterday';
      if (diffDays < 7) return `${diffDays}d ago`;
      if (diffDays < 30) return `${Math.floor(diffDays / 7)}w ago`;
      if (diffDays < 365) return `${Math.floor(diffDays / 30)}mo ago`;
      return `${Math.floor(diffDays / 365)}y ago`;
    } catch {
      return dateStr;
    }
  }

  const durationStr = $derived(formatDuration(durationSec));
  const dateStr = $derived(formatRelativeDate(video.upload_date || video.published_at));

  // Thumbnail fallback chain: maxresdefault -> hqdefault -> mqdefault
  const thumbUrls = $derived([
    `https://i.ytimg.com/vi/${videoId}/hqdefault.jpg`,
    `https://i.ytimg.com/vi/${videoId}/maxresdefault.jpg`,
    `https://i.ytimg.com/vi/${videoId}/mqdefault.jpg`,
  ]);

  const currentThumbUrl = $derived(
    video.thumb_url && thumbFallbackIndex === 0
      ? video.thumb_url
      : thumbUrls[Math.min(thumbFallbackIndex, thumbUrls.length - 1)]
  );

  function handleImageError() {
    if (thumbFallbackIndex < thumbUrls.length - 1) {
      thumbFallbackIndex++;
    }
  }

  // Get channel avatar gradient & initial
  const channelInitial = $derived(channelName.charAt(0).toUpperCase() || 'Y');

  async function handleAddToKanban() {
    if (addingToKanban || addedToKanban) return;
    addingToKanban = true;
    try {
      // Clean title of any colons (HARD LAW: Zero Colons)
      const cleanTitle = title.replace(/:/g, ' — ').trim();
      await rpc.call('kanban.create', {
        title: cleanTitle,
        channel: channelName,
        topic: video.target_keyword || title,
        target_keyword: video.target_keyword || '',
        optimal_duration_sec: durationSec > 0 ? durationSec : 720,
        notes: `Inspired by competitive research video (${videoId}) with ${formattedViews} views.`,
      });
      addedToKanban = true;
      onKanbanCreated?.();
      setTimeout(() => {
        addedToKanban = false;
      }, 3000);
    } catch (e) {
      console.error('Failed to create Kanban card:', e);
    } finally {
      addingToKanban = false;
    }
  }
</script>

<div class="group relative rounded-2xl bg-gray-900/80 border border-gray-800/80 hover:border-indigo-500/50 transition-all duration-200 overflow-hidden flex flex-col hover:shadow-2xl hover:shadow-indigo-500/10">
  
  <!-- 16:9 Thumbnail Frame -->
  <div 
    class="relative w-full aspect-video bg-gray-950 overflow-hidden cursor-pointer" 
    role="button"
    tabindex="0"
    onclick={() => onSelect?.(video)}
    onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') onSelect?.(video); }}
  >
    <img 
      src={currentThumbUrl} 
      alt={title}
      loading="lazy"
      class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
      onerror={handleImageError}
    />
    
    <!-- Dark Gradient Overlay for Readability -->
    <div class="absolute inset-0 bg-gradient-to-t from-gray-950/80 via-transparent to-black/30 pointer-events-none"></div>

    <!-- Play Icon on Hover -->
    <div class="absolute inset-0 flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity duration-200 pointer-events-none">
      <div class="w-12 h-12 rounded-full bg-indigo-600/90 text-white flex items-center justify-center shadow-xl backdrop-blur-md transform scale-90 group-hover:scale-100 transition-transform">
        <Play class="w-5 h-5 ml-0.5 fill-current" />
      </div>
    </div>

    <!-- Top Left: Outlier Multiplier Badge -->
    {#if isOutlier}
      <div class="absolute top-2.5 left-2.5 flex items-center space-x-1 px-2.5 py-1 rounded-lg bg-gradient-to-r from-amber-500 to-orange-500 text-gray-950 font-black text-xs shadow-lg backdrop-blur-sm">
        <TrendingUp class="w-3.5 h-3.5" />
        <span>{outlierMultiplier}x Breakout</span>
      </div>
    {/if}

    <!-- Top Right: Algorithmic Quality Score Badge -->
    <div class="absolute top-2.5 right-2.5 flex items-center space-x-1.5">
      {#if overallScore !== null}
        <div class="flex items-center space-x-1 px-2 py-0.5 rounded-md bg-indigo-950/90 text-indigo-300 border border-indigo-500/40 text-[11px] font-mono font-bold backdrop-blur-md shadow-md">
          <Sparkles class="w-3 h-3 text-indigo-400" />
          <span>{overallScore.toFixed(0)}</span>
        </div>
      {:else if seoScore !== null}
        <div class="flex items-center space-x-1 px-2 py-0.5 rounded-md bg-gray-950/90 text-indigo-400 border border-indigo-500/30 text-[11px] font-mono font-medium backdrop-blur-md">
          <span>{seoScore.toFixed(0)} SEO</span>
        </div>
      {/if}
    </div>

    <!-- Bottom Right: Video Duration Overlay (YouTube Standard) -->
    {#if durationStr}
      <div class="absolute bottom-2.5 right-2.5 px-2 py-0.5 rounded-md bg-gray-950/90 text-white font-mono text-[11px] font-bold tracking-tight shadow-md border border-white/10 backdrop-blur-sm">
        {durationStr}
      </div>
    {/if}

    <!-- Bottom Left: Upload Age -->
    {#if dateStr}
      <div class="absolute bottom-2.5 left-2.5 flex items-center space-x-1 px-2 py-0.5 rounded-md bg-gray-950/80 text-gray-300 text-[11px] font-medium backdrop-blur-sm border border-white/5">
        <Clock class="w-3 h-3 text-gray-400" />
        <span>{dateStr}</span>
      </div>
    {/if}
  </div>

  <!-- Content Body -->
  <div class="p-4 flex-1 flex flex-col justify-between space-y-3">
    
    <!-- Channel Header & Video Title -->
    <div class="space-y-2">
      <div class="flex items-center space-x-2.5">
        <!-- Channel Avatar Initial Badge -->
        <div class="w-6 h-6 rounded-full bg-gradient-to-tr from-indigo-600 to-purple-500 flex items-center justify-center text-[10px] font-bold text-white shadow-sm ring-1 ring-white/10 shrink-0">
          {channelInitial}
        </div>
        <span class="text-xs font-semibold text-indigo-400 hover:text-indigo-300 truncate max-w-[200px]" title={channelName}>
          {channelName}
        </span>
      </div>

      <button 
        type="button"
        class="text-left text-sm font-semibold text-gray-100 line-clamp-2 leading-snug group-hover:text-indigo-300 transition-colors cursor-pointer w-full focus:outline-none"
        onclick={() => onSelect?.(video)}
        title={title}
      >
        {title}
      </button>
    </div>

    <!-- Stats & Quick Actions Toolbar -->
    <div class="pt-3 border-t border-gray-800/80 flex items-center justify-between gap-2">
      
      <!-- Views & Likes Counters -->
      <div class="flex items-center space-x-2.5 text-xs text-gray-400 font-mono">
        <span class="flex items-center space-x-1" title={`${views.toLocaleString()} views`}>
          <Eye class="w-3.5 h-3.5 text-gray-500" />
          <span class="font-medium text-gray-300">{formattedViews}</span>
        </span>
        {#if likes > 0}
          <span class="flex items-center space-x-1" title={`${likes.toLocaleString()} likes`}>
            <ThumbsUp class="w-3.5 h-3.5 text-gray-500" />
            <span>{formattedLikes}</span>
          </span>
        {/if}
      </div>

      <!-- Action Buttons -->
      <div class="flex items-center space-x-1.5">
        
        <!-- Dedicated Analytics Deep-Dive Button -->
        <button
          type="button"
          onclick={() => onSelect?.(video)}
          class="inline-flex items-center space-x-1 px-2 py-1 rounded-lg bg-indigo-500/10 hover:bg-indigo-600 text-indigo-300 hover:text-white border border-indigo-500/20 text-xs font-bold transition-all cursor-pointer"
          title="Inspect 17 SEO & 7 GEO Deep-Dive Video Analytics"
        >
          <BarChart3 class="w-3.5 h-3.5 text-indigo-400" />
          <span class="text-[10px] hidden xs:inline">Analytics</span>
        </button>

        <!-- 1-Click Kanban Creation Button -->
        <button
          type="button"
          onclick={handleAddToKanban}
          disabled={addingToKanban || addedToKanban}
          class="p-1.5 rounded-lg text-xs transition-colors cursor-pointer {addedToKanban ? 'bg-emerald-500/20 text-emerald-400 border border-emerald-500/30' : 'bg-gray-800 hover:bg-indigo-600/30 text-gray-300 hover:text-indigo-300 border border-gray-700/60'}"
          title={addedToKanban ? 'Added to Kanban!' : 'Add to Production Kanban'}
        >
          {#if addedToKanban}
            <Check class="w-3.5 h-3.5" />
          {:else}
            <Kanban class="w-3.5 h-3.5" />
          {/if}
        </button>

        {#if onInspectGaps}
          <button
            type="button"
            onclick={() => onInspectGaps(video)}
            class="p-1.5 rounded-lg bg-gray-800 hover:bg-gray-700 text-gray-300 hover:text-white border border-gray-700/60 transition-colors cursor-pointer"
            title="Inspect Topic Gaps & Semantic Keywords"
          >
            <Search class="w-3.5 h-3.5" />
          </button>
        {/if}

        <!-- Direct YouTube Watch Link -->
        <a 
          href={`https://www.youtube.com/watch?v=${videoId}`}
          target="_blank"
          rel="noopener noreferrer"
          class="p-1.5 rounded-lg bg-gray-800 hover:bg-red-500/20 text-gray-300 hover:text-red-400 border border-gray-700/60 transition-colors"
          title="Watch on YouTube"
        >
          <ExternalLink class="w-3.5 h-3.5" />
        </a>

      </div>

    </div>

  </div>
</div>

