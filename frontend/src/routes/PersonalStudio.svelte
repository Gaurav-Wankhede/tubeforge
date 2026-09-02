<script lang="ts">
  import { onMount } from 'svelte';
  import { 
    Tv, 
    Plus, 
    Trash2, 
    Sparkles, 
    TrendingUp, 
    Clock, 
    Tag, 
    Check, 
    AlertTriangle, 
    ExternalLink, 
    Video, 
    BarChart3, 
    ShieldAlert, 
    Lightbulb, 
    Layers, 
    Radio, 
    Kanban, 
    Copy,
    ChevronRight,
    Users,
    Eye,
    ThumbsUp,
    MessageSquare,
    Calendar,
    Play,
    Info,
    ChevronDown,
    ChevronUp
  } from 'lucide-svelte';
  import VideoAnalyticsModal from '../components/VideoAnalyticsModal.svelte';

  interface UserChannel {
    channel_id: string;
    title: string;
    handle: string | null;
    description: string;
    subscriber_count: number;
    video_count: number;
    total_views: number;
    avg_views: number;
    avatar_url: string | null;
    is_primary: boolean;
    created_at: string;
  }

  interface VideoItem {
    video_id: string;
    title: string;
    description?: string;
    thumb_url: string;
    view_count: number;
    duration_sec: number;
    like_count: number;
    comment_count: number;
    published_at: string;
    tags: string[];
    updated_at: string;
    eda?: {
      outlier_multiplier: number;
      outlier_label: string;
      engagement_density: number;
      expected_watch_hours: number;
      is_mobile_safe: boolean;
      mobile_preview: string;
      char_count: number;
      has_colon: boolean;
    };
  }

  interface Improvement {
    id: string;
    priority: 'HIGH' | 'MEDIUM' | 'QUICK_WIN';
    category: string;
    title: string;
    description: string;
    action_type: string;
    action_label: string;
  }

  interface AnalysisData {
    channel: {
      channel_id: string;
      title: string;
      handle: string | null;
      description: string;
      custom_name: string | null;
      is_primary: boolean;
      subscriber_count: number;
      avatar_url: string | null;
      video_count: number;
      total_views: number;
      avg_views: number;
      median_views?: number;
      avg_duration_sec: number;
      avg_title_length: number;
      titles_with_colons: number;
    };
    competitor_benchmark: {
      channel_count: number;
      video_count: number;
      avg_views: number;
      avg_duration_sec: number;
    };
    missing_tags: Array<{ tag: string; competitor_occurrences: number }>;
    improvements: Improvement[];
    videos: VideoItem[];
  }

  let channels = $state<UserChannel[]>([]);
  let selectedChannelId = $state<string | null>(null);
  let analysis = $state<AnalysisData | null>(null);
  let loadingChannels = $state(true);
  let loadingAnalysis = $state(false);
  let refreshingVideos = $state(false);

  // Add Channel Modal State
  let showAddModal = $state(false);
  let addChannelInput = $state('');
  let addChannelName = $state('');
  let addingChannel = $state(false);
  let addError = $state<string | null>(null);
  let addSuccess = $state<string | null>(null);

  // Description expand state
  let showFullDesc = $state(false);

  // Video Analytics Modal State
  let selectedVideoId = $state<string | null>(null);

  // Copy Feedback
  let copiedTag = $state<string | null>(null);

  let { onNavigate }: { onNavigate?: (route: string, param?: string) => void } = $props();

  async function loadUserChannels() {
    loadingChannels = true;
    try {
      const res = await fetch('/api/user/channels');
      if (res.ok) {
        const data = await res.json();
        channels = data.channels || [];
        if (channels.length > 0 && (!selectedChannelId || !channels.some(c => c.channel_id === selectedChannelId))) {
          selectedChannelId = channels[0].channel_id;
        }
      }
    } catch {
      // ignore
    } finally {
      loadingChannels = false;
    }
  }

  async function loadChannelAnalysis(channelId: string) {
    if (!channelId) return;
    loadingAnalysis = true;
    showFullDesc = false;
    try {
      const res = await fetch(`/api/user/channels/${channelId}/analysis`);
      if (res.ok) {
        analysis = await res.json();
      }
    } catch {
      // ignore
    } finally {
      loadingAnalysis = false;
    }
  }

  async function handleRefreshChannelVideos() {
    if (!selectedChannelId) return;
    refreshingVideos = true;
    try {
      await fetch(`/api/user/channels/${selectedChannelId}/refresh`, { method: 'POST' });
      setTimeout(async () => {
        if (selectedChannelId) {
          await loadChannelAnalysis(selectedChannelId);
        }
        refreshingVideos = false;
      }, 2500);
    } catch {
      refreshingVideos = false;
    }
  }

  $effect(() => {
    if (selectedChannelId) {
      loadChannelAnalysis(selectedChannelId);
    }
  });

  onMount(() => {
    loadUserChannels();
  });

  async function handleAddChannel() {
    if (!addChannelInput.trim()) return;
    addingChannel = true;
    addError = null;
    addSuccess = null;

    try {
      const params = new URLSearchParams({
        input: addChannelInput.trim(),
      });
      if (addChannelName.trim()) {
        params.append('custom_name', addChannelName.trim());
      }

      const res = await fetch(`/api/user/channels?${params.toString()}`, {
        method: 'POST',
      });
      const data = await res.json();

      if (!res.ok || data.error) {
        addError = data.error || 'Failed to resolve YouTube channel';
      } else {
        addSuccess = data.message || 'Channel added successfully!';
        addChannelInput = '';
        addChannelName = '';
        await loadUserChannels();
        if (data.channel && data.channel.channel_id) {
          selectedChannelId = data.channel.channel_id;
        }
        setTimeout(() => {
          showAddModal = false;
          addSuccess = null;
        }, 1200);
      }
    } catch (e: any) {
      addError = e.message || 'Network error while adding channel';
    } finally {
      addingChannel = false;
    }
  }

  async function handleDeleteChannel(channelId: string) {
    if (!confirm('Are you sure you want to remove this channel from your Personal Studio? (Videos in the global database will be preserved)')) {
      return;
    }

    try {
      const res = await fetch(`/api/user/channels/delete?channel_id=${channelId}`, {
        method: 'POST',
      });
      if (res.ok) {
        await loadUserChannels();
      }
    } catch {}
  }

  function copyToClipboard(text: string) {
    navigator.clipboard.writeText(text);
    copiedTag = text;
    setTimeout(() => copiedTag = null, 1500);
  }

  function formatDuration(seconds: number): string {
    if (!seconds) return '0:00';
    const m = Math.floor(seconds / 60);
    const s = seconds % 60;
    return `${m}:${s < 10 ? '0' : ''}${s}`;
  }

  function formatDate(dateStr: string): string {
    if (!dateStr) return '';
    try {
      const d = new Date(dateStr);
      return d.toLocaleDateString('en-US', { year: 'numeric', month: 'short', day: 'numeric' });
    } catch {
      return dateStr.slice(0, 10);
    }
  }
</script>

<div class="space-y-6">

  <!-- Header & Channel Switcher Bar -->
  <div class="p-6 rounded-2xl bg-gray-900/60 border border-gray-800 space-y-5">
    
    <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
      <div class="space-y-1">
        <div class="flex items-center space-x-2.5">
          <div class="p-2 rounded-xl bg-indigo-500/10 border border-indigo-500/20 text-indigo-400">
            <Tv class="w-5 h-5" />
          </div>
          <div>
            <h2 class="text-xl font-extrabold text-white tracking-tight">
              Personal Channel Studio & Intelligence
            </h2>
            <p class="text-xs text-gray-400">
              Isolated channel telemetry, full HD media metadata, competitor benchmarks, and actionable improvements.
            </p>
          </div>
        </div>
      </div>

      <button
        type="button"
        onclick={() => showAddModal = true}
        class="inline-flex items-center space-x-2 px-4 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-bold shadow-lg shadow-indigo-500/20 transition-all cursor-pointer shrink-0"
      >
        <Plus class="w-4 h-4" />
        <span>Add Your Channel</span>
      </button>
    </div>

    <!-- Channel Selector Tabs -->
    {#if loadingChannels}
      <div class="flex space-x-3 animate-pulse">
        <div class="h-10 w-48 bg-gray-800/60 rounded-xl"></div>
        <div class="h-10 w-48 bg-gray-800/60 rounded-xl"></div>
      </div>
    {:else if channels.length === 0}
      <div class="p-6 rounded-xl bg-gray-950/80 border border-gray-800/80 text-center space-y-3">
        <p class="text-sm text-gray-300 font-medium">No personal channels added yet.</p>
        <p class="text-xs text-gray-500">Add your YouTube channel URL or @handle to unlock personalized competitive gap analysis.</p>
        <button
          type="button"
          onclick={() => showAddModal = true}
          class="px-4 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-bold transition-all cursor-pointer"
        >
          Add Your First Channel
        </button>
      </div>
    {:else}
      <div class="flex items-center space-x-3 overflow-x-auto pb-1 pt-1">
        {#each channels as chan}
          <div class="relative group shrink-0">
            <button
              type="button"
              onclick={() => selectedChannelId = chan.channel_id}
              class="flex items-center space-x-3 px-4 py-2.5 rounded-xl text-xs font-bold transition-all cursor-pointer border {
                selectedChannelId === chan.channel_id
                  ? 'bg-indigo-600/20 border-indigo-500 text-white shadow-lg shadow-indigo-500/10'
                  : 'bg-gray-950/60 border-gray-800/80 text-gray-400 hover:text-gray-200 hover:border-gray-700'
              }"
            >
              {#if chan.avatar_url}
                <img src={chan.avatar_url} alt={chan.title} class="w-6 h-6 rounded-full object-cover border border-gray-700 shadow-sm" />
              {:else}
                <div class="w-6 h-6 rounded-full bg-indigo-500/20 text-indigo-300 flex items-center justify-center text-xs">
                  {chan.title.charAt(0)}
                </div>
              {/if}

              <span>{chan.title}</span>

              {#if chan.handle}
                <span class="text-[10px] font-mono font-normal opacity-75 text-indigo-300">
                  {chan.handle}
                </span>
              {/if}

              <span class="px-2 py-0.5 rounded-full text-[10px] font-mono {selectedChannelId === chan.channel_id ? 'bg-indigo-500/40 text-indigo-100 font-bold' : 'bg-gray-800 text-gray-400'}">
                {chan.video_count} vids
              </span>
            </button>

            <!-- Delete Channel Button on Hover -->
            <button
              type="button"
              onclick={(e) => { e.stopPropagation(); handleDeleteChannel(chan.channel_id); }}
              class="absolute -top-1.5 -right-1.5 p-1 rounded-full bg-gray-900 border border-gray-700 text-gray-400 hover:text-rose-400 hover:border-rose-500/50 opacity-0 group-hover:opacity-100 transition-opacity cursor-pointer shadow-md"
              title="Remove channel from studio"
            >
              <Trash2 class="w-3 h-3" />
            </button>
          </div>
        {/each}
      </div>
    {/if}

  </div>

  <!-- Analysis Content for Selected Channel -->
  {#if loadingAnalysis}
    <div class="p-16 text-center space-y-3">
      <div class="animate-spin w-8 h-8 border-2 border-indigo-500 border-t-transparent rounded-full mx-auto"></div>
      <p class="text-xs text-gray-400 font-mono">Loading full channel metadata, media assets & competitive gap benchmarks...</p>
    </div>
  {:else if analysis}
    
    <!-- 1. Channel Profile Hero Card -->
    <div class="p-6 rounded-2xl bg-gradient-to-br from-gray-900 via-gray-900/90 to-gray-950 border border-gray-800 shadow-xl space-y-4">
      <div class="flex flex-col md:flex-row md:items-center justify-between gap-5">
        
        <div class="flex items-center space-x-4">
          <!-- Large Channel Avatar -->
          <div class="relative shrink-0">
            {#if analysis.channel.avatar_url}
              <img 
                src={analysis.channel.avatar_url} 
                alt={analysis.channel.title} 
                class="w-16 h-16 sm:w-20 sm:h-20 rounded-2xl object-cover border-2 border-indigo-500/40 shadow-xl"
              />
            {:else}
              <div class="w-16 h-16 sm:w-20 sm:h-20 rounded-2xl bg-indigo-500/20 border-2 border-indigo-500/40 text-indigo-300 flex items-center justify-center text-2xl font-bold">
                {analysis.channel.title.charAt(0)}
              </div>
            {/if}
            <span class="absolute -bottom-1 -right-1 p-1 rounded-full bg-indigo-600 text-white shadow-md" title="Tracked Channel">
              <Check class="w-3 h-3" />
            </span>
          </div>

          <!-- Channel Titles & Metadata Badges -->
          <div class="space-y-1.5">
            <div class="flex flex-wrap items-center gap-2">
              <h3 class="text-xl sm:text-2xl font-black text-white tracking-tight">
                {analysis.channel.title}
              </h3>
              {#if analysis.channel.is_primary}
                <span class="px-2 py-0.5 rounded-md bg-indigo-500/20 text-indigo-300 text-[10px] font-mono font-bold border border-indigo-500/30">
                  PRIMARY
                </span>
              {/if}
              {#if analysis.channel.custom_name}
                <span class="px-2 py-0.5 rounded-md bg-purple-500/20 text-purple-300 text-[10px] font-mono font-bold border border-purple-500/30">
                  {analysis.channel.custom_name}
                </span>
              {/if}
            </div>

            <div class="flex flex-wrap items-center gap-3 text-xs text-gray-400 font-mono">
              {#if analysis.channel.handle}
                <span class="text-indigo-400 font-semibold">{analysis.channel.handle}</span>
                <span>•</span>
              {/if}
              <span>ID: {analysis.channel.channel_id}</span>
              <span>•</span>
              <span>{analysis.channel.video_count} Ingested Videos</span>
            </div>
          </div>
        </div>

        <!-- Action Buttons -->
        <div class="shrink-0 flex items-center space-x-2">
          <button
            type="button"
            disabled={refreshingVideos}
            onclick={() => handleRefreshChannelVideos()}
            class="inline-flex items-center space-x-2 px-4 py-2.5 rounded-xl bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 text-white text-xs font-bold transition-all shadow-md shadow-indigo-500/20 cursor-pointer"
          >
            {#if refreshingVideos}
              <span class="animate-spin w-3.5 h-3.5 border-2 border-white border-t-transparent rounded-full"></span>
              <span>Syncing Live Stats...</span>
            {:else}
              <Sparkles class="w-3.5 h-3.5" />
              <span>Live Sync Video Stats</span>
            {/if}
          </button>

          <a
            href={analysis.channel.handle ? `https://youtube.com/${analysis.channel.handle}` : `https://youtube.com/channel/${analysis.channel.channel_id}`}
            target="_blank"
            rel="noopener noreferrer"
            class="inline-flex items-center space-x-2 px-4 py-2.5 rounded-xl bg-gray-800/80 hover:bg-gray-750 border border-gray-700 text-gray-200 text-xs font-bold transition-all shadow-md cursor-pointer"
          >
            <span>View on YouTube</span>
            <ExternalLink class="w-3.5 h-3.5 text-gray-400" />
          </a>
        </div>

      </div>

      <!-- Expandable Channel Description -->
      {#if analysis.channel.description}
        <div class="pt-3 border-t border-gray-800/80 space-y-1">
          <div class="flex items-center justify-between">
            <span class="text-[11px] font-mono text-gray-400 uppercase tracking-wider">Channel Mission & Bio</span>
            <button
              type="button"
              onclick={() => showFullDesc = !showFullDesc}
              class="text-[11px] text-indigo-400 hover:text-indigo-300 inline-flex items-center space-x-1 cursor-pointer"
            >
              <span>{showFullDesc ? 'Show Less' : 'Read Full Bio'}</span>
              {#if showFullDesc}
                <ChevronUp class="w-3 h-3" />
              {:else}
                <ChevronDown class="w-3 h-3" />
              {/if}
            </button>
          </div>
          <p class="text-xs text-gray-300 leading-relaxed font-sans {showFullDesc ? 'whitespace-pre-wrap' : 'line-clamp-2'}">
            {analysis.channel.description}
          </p>
        </div>
      {/if}
    </div>

    <!-- 2. Channel Telemetry Overview Cards -->
    <div class="grid grid-cols-2 sm:grid-cols-4 gap-4">
      
      <div class="p-4 rounded-2xl bg-gray-900/60 border border-gray-800 space-y-1">
        <div class="flex items-center justify-between text-gray-400">
          <span class="text-xs font-medium">Channel Videos</span>
          <Video class="w-4 h-4 text-indigo-400" />
        </div>
        <div class="text-2xl font-extrabold text-white font-mono">
          {analysis.channel.video_count}
        </div>
        <span class="text-[10px] text-gray-500 block">
          Ingested into SQLite database
        </span>
      </div>

      <div class="p-4 rounded-2xl bg-gray-900/60 border border-gray-800 space-y-1">
        <div class="flex items-center justify-between text-gray-400">
          <span class="text-xs font-medium">Total Channel Views</span>
          <Eye class="w-4 h-4 text-emerald-400" />
        </div>
        <div class="text-2xl font-extrabold text-emerald-400 font-mono">
          {analysis.channel.total_views.toLocaleString()}
        </div>
        <span class="text-[10px] text-gray-500 block">
          Live sync across all videos
        </span>
      </div>

      <div class="p-4 rounded-2xl bg-gray-900/60 border border-gray-800 space-y-1">
        <div class="flex items-center justify-between text-gray-400">
          <span class="text-xs font-medium">Avg Views / Video</span>
          <TrendingUp class="w-4 h-4 text-purple-400" />
        </div>
        <div class="text-2xl font-extrabold text-purple-400 font-mono">
          {analysis.channel.avg_views.toLocaleString()}
        </div>
        <span class="text-[10px] text-gray-500 block">
          Channel baseline mean
        </span>
      </div>

      <div class="p-4 rounded-2xl bg-gray-900/60 border border-gray-800 space-y-1">
        <div class="flex items-center justify-between text-gray-400">
          <span class="text-xs font-medium">Avg Duration</span>
          <Clock class="w-4 h-4 text-amber-400" />
        </div>
        <div class="text-2xl font-extrabold text-amber-400 font-mono">
          {formatDuration(analysis.channel.avg_duration_sec)}
        </div>
        <span class="text-[10px] text-gray-500 block">
          Target: 8:00 – 14:00 min
        </span>
      </div>

    </div>

    <!-- 3. Benchmark Comparison: Your Channel vs Ingested Competitors -->
    <div class="p-6 rounded-2xl bg-gray-900/60 border border-gray-800 space-y-4">
      <div class="flex items-center justify-between">
        <div>
          <h3 class="text-sm font-bold text-white uppercase tracking-wider flex items-center space-x-2">
            <BarChart3 class="w-4 h-4 text-indigo-400" />
            <span>Competitive Benchmark Matrix (vs {analysis.competitor_benchmark.channel_count} Competitor Channels)</span>
          </h3>
          <p class="text-xs text-gray-400 mt-0.5">
            Comparing your channel's metrics against {analysis.competitor_benchmark.video_count.toLocaleString()} ingested competitor videos.
          </p>
        </div>
      </div>

      <div class="grid grid-cols-1 md:grid-cols-3 gap-4 pt-2">
        
        <!-- Metric 1: Average Views -->
        <div class="p-4 rounded-xl bg-gray-950/80 border border-gray-800/80 space-y-2">
          <span class="text-xs font-semibold text-gray-400">Average View Depth</span>
          <div class="flex justify-between items-baseline font-mono text-sm">
            <span class="text-indigo-300 font-bold">You: {analysis.channel.avg_views.toLocaleString()}</span>
            <span class="text-gray-500">Competitors: {analysis.competitor_benchmark.avg_views.toLocaleString()}</span>
          </div>
          <div class="h-2 w-full bg-gray-800 rounded-full overflow-hidden">
            <div 
              class="h-full bg-indigo-500 rounded-full"
              style={`width: ${Math.min(Math.round((analysis.channel.avg_views / Math.max(analysis.competitor_benchmark.avg_views, 1)) * 100), 100)}%`}
            ></div>
          </div>
        </div>

        <!-- Metric 2: Average Duration -->
        <div class="p-4 rounded-xl bg-gray-950/80 border border-gray-800/80 space-y-2">
          <span class="text-xs font-semibold text-gray-400">Duration Sweet Spot</span>
          <div class="flex justify-between items-baseline font-mono text-sm">
            <span class="text-purple-300 font-bold">You: {formatDuration(analysis.channel.avg_duration_sec)}</span>
            <span class="text-gray-500">Competitors: {formatDuration(analysis.competitor_benchmark.avg_duration_sec)}</span>
          </div>
          <div class="h-2 w-full bg-gray-800 rounded-full overflow-hidden">
            <div 
              class="h-full bg-purple-500 rounded-full"
              style={`width: ${Math.min(Math.round((analysis.channel.avg_duration_sec / Math.max(analysis.competitor_benchmark.avg_duration_sec, 1)) * 100), 100)}%`}
            ></div>
          </div>
        </div>

        <!-- Metric 3: Title Hook Hygiene -->
        <div class="p-4 rounded-xl bg-gray-950/80 border border-gray-800/80 space-y-2">
          <span class="text-xs font-semibold text-gray-400">Zero-Colon Title Rule</span>
          <div class="flex justify-between items-baseline font-mono text-sm">
            <span class="{analysis.channel.titles_with_colons === 0 ? 'text-emerald-400' : 'text-amber-400'} font-bold">
              {analysis.channel.titles_with_colons === 0 ? '✓ 100% Clean' : `${analysis.channel.titles_with_colons} Colons Found`}
            </span>
            <span class="text-gray-500">Avg Length: {analysis.channel.avg_title_length} chars</span>
          </div>
          <div class="h-2 w-full bg-gray-800 rounded-full overflow-hidden">
            <div 
              class="h-full {analysis.channel.titles_with_colons === 0 ? 'bg-emerald-500' : 'bg-amber-500'} rounded-full"
              style={`width: ${analysis.channel.titles_with_colons === 0 ? '100' : '40'}%`}
            ></div>
          </div>
        </div>

      </div>
    </div>

    <!-- 4. Prescriptive Improvements Engine -->
    <div class="p-6 rounded-2xl bg-gray-900/60 border border-gray-800 space-y-4">
      <div class="flex items-center space-x-2">
        <Sparkles class="w-4 h-4 text-amber-400" />
        <h3 class="text-sm font-bold text-white uppercase tracking-wider">
          What Improvements Are Needed (Prescriptive Action Plan)
        </h3>
      </div>

      <div class="grid grid-cols-1 md:grid-cols-2 gap-4 pt-1">
        {#each analysis.improvements as imp}
          <div class="p-4 rounded-xl bg-gray-950/80 border border-gray-800 space-y-3 flex flex-col justify-between">
            <div class="space-y-1.5">
              <div class="flex items-center justify-between">
                <span class="px-2 py-0.5 rounded-full text-[10px] font-mono font-bold {
                  imp.priority === 'HIGH' ? 'bg-rose-500/10 text-rose-400 border border-rose-500/20' :
                  imp.priority === 'MEDIUM' ? 'bg-amber-500/10 text-amber-400 border border-amber-500/20' :
                  'bg-emerald-500/10 text-emerald-400 border border-emerald-500/20'
                }">
                  {imp.priority} PRIORITY · {imp.category}
                </span>
              </div>
              <h4 class="text-sm font-bold text-white">
                {imp.title}
              </h4>
              <p class="text-xs text-gray-400 leading-relaxed">
                {imp.description}
              </p>
            </div>

            <div class="pt-2 border-t border-gray-900 flex justify-end">
              {#if imp.action_type === 'kanban' && onNavigate}
                <button
                  type="button"
                  onclick={() => onNavigate('kanban')}
                  class="inline-flex items-center space-x-1.5 text-xs font-bold text-indigo-400 hover:text-indigo-300 transition-colors cursor-pointer"
                >
                  <Kanban class="w-3.5 h-3.5" />
                  <span>{imp.action_label} →</span>
                </button>
              {:else if imp.action_type === 'teleprompter' && onNavigate}
                <button
                  type="button"
                  onclick={() => onNavigate('teleprompter')}
                  class="inline-flex items-center space-x-1.5 text-xs font-bold text-purple-400 hover:text-purple-300 transition-colors cursor-pointer"
                >
                  <Radio class="w-3.5 h-3.5" />
                  <span>{imp.action_label} →</span>
                </button>
              {:else if imp.action_type === 'research' && onNavigate}
                <button
                  type="button"
                  onclick={() => onNavigate('research', analysis?.channel.title || '')}
                  class="inline-flex items-center space-x-1.5 text-xs font-bold text-emerald-400 hover:text-emerald-300 transition-colors cursor-pointer"
                >
                  <Sparkles class="w-3.5 h-3.5" />
                  <span>{imp.action_label} →</span>
                </button>
              {:else}
                <span class="text-[11px] text-gray-500 font-mono">
                  {imp.action_label}
                </span>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    </div>

    <!-- 5. Missing High-Impact Competitor Tags Cloud -->
    {#if analysis.missing_tags && analysis.missing_tags.length > 0}
      <div class="p-6 rounded-2xl bg-gray-900/60 border border-gray-800 space-y-4">
        <div class="flex items-center justify-between">
          <div class="space-y-0.5">
            <h3 class="text-sm font-bold text-white uppercase tracking-wider flex items-center space-x-2">
              <Tag class="w-4 h-4 text-emerald-400" />
              <span>Missing High-Performing Competitor Tags</span>
            </h3>
            <p class="text-xs text-gray-400">
              Top tags ranked by competitors in your niche that your channel has not targeted. Click any tag to copy.
            </p>
          </div>
        </div>

        <div class="flex flex-wrap gap-2 pt-2">
          {#each analysis.missing_tags as item}
            <button
              type="button"
              onclick={() => copyToClipboard(item.tag)}
              class="inline-flex items-center space-x-1.5 px-3 py-1.5 rounded-xl text-xs font-medium bg-gray-950/80 border border-gray-800 hover:border-emerald-500/40 text-gray-300 hover:text-emerald-300 transition-all cursor-pointer group"
              title={`Used by ${item.competitor_occurrences} competitor videos`}
            >
              <span>{item.tag}</span>
              <span class="text-[10px] font-mono text-gray-500 group-hover:text-emerald-400">
                ({item.competitor_occurrences})
              </span>
              {#if copiedTag === item.tag}
                <Check class="w-3 h-3 text-emerald-400" />
              {:else}
                <Copy class="w-3 h-3 opacity-0 group-hover:opacity-100 text-gray-400 transition-opacity" />
              {/if}
            </button>
          {/each}
        </div>
      </div>
    {/if}

    <!-- 6. Channel Ingested Videos Gallery (Full Metadata + HD Thumbnail Images) -->
    <div class="p-6 rounded-2xl bg-gray-900/60 border border-gray-800 space-y-5">
      <div class="flex items-center justify-between">
        <div class="space-y-0.5">
          <h3 class="text-sm font-bold text-white uppercase tracking-wider flex items-center space-x-2">
            <Video class="w-4 h-4 text-indigo-400" />
            <span>Channel Video Gallery ({analysis.videos.length} Ingested Videos)</span>
          </h3>
          <p class="text-xs text-gray-400">
            Full metadata, live views, engagement stats, HD thumbnails, and semantic tags.
          </p>
        </div>
      </div>

      {#if analysis.videos.length === 0}
        <div class="p-12 text-center text-xs text-gray-500 bg-gray-950/60 rounded-xl border border-gray-800">
          No videos ingested for this channel yet. Click "Sync Live Stats" in the header to run metadata ingestion.
        </div>
      {:else}
        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-5">
          {#each analysis.videos as vid}
            <div 
              class="rounded-2xl bg-gray-950/90 border border-gray-800/90 hover:border-indigo-500/50 hover:shadow-2xl hover:shadow-indigo-500/10 transition-all overflow-hidden flex flex-col justify-between group"
            >
              
              <!-- Video Thumbnail Container -->
              <div class="relative aspect-video bg-gray-900 overflow-hidden">
                <img 
                  src={vid.thumb_url} 
                  alt={vid.title} 
                  loading="lazy"
                  class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
                  onerror={(e) => {
                    const target = e.currentTarget as HTMLImageElement;
                    if (!target.src.includes('hqdefault.jpg')) {
                      target.src = `https://i.ytimg.com/vi/${vid.video_id}/hqdefault.jpg`;
                    }
                  }}
                />

                <!-- Duration Overlay -->
                {#if vid.duration_sec > 0}
                  <div class="absolute bottom-2 right-2 px-2 py-0.5 rounded-md bg-black/85 backdrop-blur-md text-[10px] font-mono font-bold text-white">
                    {formatDuration(vid.duration_sec)}
                  </div>
                {/if}

                <!-- Colon Warning Overlay -->
                {#if vid.title.includes(':')}
                  <div class="absolute top-2 left-2 px-2 py-0.5 rounded-md bg-rose-600/90 backdrop-blur-md text-[9px] font-mono font-bold text-white flex items-center space-x-1 shadow-md">
                    <AlertTriangle class="w-3 h-3" />
                    <span>COLON DETECTED</span>
                  </div>
                {/if}

                <!-- Hover Play / Analytics Button -->
                <button
                  type="button"
                  onclick={() => selectedVideoId = vid.video_id}
                  class="absolute inset-0 bg-black/40 opacity-0 group-hover:opacity-100 backdrop-blur-[2px] transition-opacity flex items-center justify-center space-x-2 text-white text-xs font-bold cursor-pointer"
                >
                  <div class="p-2.5 rounded-full bg-indigo-600 text-white shadow-xl">
                    <Play class="w-4 h-4 fill-white" />
                  </div>
                  <span>Inspect Analytics</span>
                </button>
              </div>

              <!-- Video Content Info -->
              <div class="p-4 space-y-3 flex-1 flex flex-col justify-between">
                
                <div class="space-y-2">
                  <button 
                    type="button"
                    onclick={() => selectedVideoId = vid.video_id}
                    class="text-left text-xs font-bold text-gray-200 group-hover:text-indigo-300 transition-colors line-clamp-2 leading-snug cursor-pointer" 
                    title={vid.title}
                  >
                    {vid.title}
                  </button>

                  <!-- Engagement Stats Row -->
                  <div class="flex items-center justify-between text-xs font-mono pt-1 text-gray-400">
                    <div class="flex items-center space-x-1 text-emerald-400 font-bold">
                      <Eye class="w-3.5 h-3.5" />
                      <span>{vid.view_count.toLocaleString()}</span>
                    </div>

                    {#if vid.like_count > 0}
                      <div class="flex items-center space-x-1 text-indigo-300">
                        <ThumbsUp class="w-3 h-3" />
                        <span>{vid.like_count.toLocaleString()}</span>
                      </div>
                    {/if}

                    {#if vid.published_at}
                      <div class="flex items-center space-x-1 text-gray-500 text-[10px]">
                        <Calendar class="w-3 h-3" />
                        <span>{formatDate(vid.published_at)}</span>
                      </div>
                    {/if}
                  </div>

                  <!-- Mathematical EDA Metrics Badges -->
                  {#if vid.eda}
                    <div class="flex items-center justify-between pt-1 text-[10px] font-mono">
                      {#if vid.eda.outlier_multiplier >= 3.0}
                        <span class="px-2 py-0.5 rounded-md bg-rose-500/20 text-rose-300 font-bold border border-rose-500/30">
                          🔥 {vid.eda.outlier_multiplier}x Breakout
                        </span>
                      {:else if vid.eda.outlier_multiplier >= 1.5}
                        <span class="px-2 py-0.5 rounded-md bg-amber-500/20 text-amber-300 font-bold border border-amber-500/30">
                          ⚡ {vid.eda.outlier_multiplier}x High Resonance
                        </span>
                      {:else}
                        <span class="px-2 py-0.5 rounded-md bg-gray-900 text-gray-400 border border-gray-800">
                          ⚖️ {vid.eda.outlier_multiplier}x Baseline
                        </span>
                      {/if}

                      <span class="text-purple-300 font-semibold" title="Engagement Density">
                        Density: {vid.eda.engagement_density}
                      </span>
                    </div>
                  {/if}
                </div>

                <!-- Video Tags -->
                <div class="space-y-2 pt-2 border-t border-gray-900">
                  {#if vid.tags && vid.tags.length > 0}
                    <div class="flex flex-wrap gap-1">
                      {#each vid.tags.slice(0, 3) as tag}
                        <span class="px-1.5 py-0.5 rounded-md bg-gray-900 border border-gray-800 text-[10px] text-gray-400 truncate max-w-[130px]">
                          {tag}
                        </span>
                      {/each}
                      {#if vid.tags.length > 3}
                        <span class="px-1.5 py-0.5 rounded-md bg-gray-900 text-[10px] text-gray-500 font-mono">
                          +{vid.tags.length - 3}
                        </span>
                      {/if}
                    </div>
                  {:else}
                    <span class="text-[10px] text-gray-600 italic block">No tags extracted</span>
                  {/if}

                  <!-- Quick Action Links -->
                  <div class="flex items-center justify-between pt-1 text-[11px] font-mono">
                    <button
                      type="button"
                      onclick={() => selectedVideoId = vid.video_id}
                      class="text-indigo-400 hover:text-indigo-300 font-semibold cursor-pointer"
                    >
                      Deep Analytics →
                    </button>

                    <a
                      href={`https://youtube.com/watch?v=${vid.video_id}`}
                      target="_blank"
                      rel="noopener noreferrer"
                      class="text-gray-500 hover:text-gray-300 inline-flex items-center space-x-1"
                      title="Open on YouTube"
                    >
                      <span>Watch</span>
                      <ExternalLink class="w-3 h-3" />
                    </a>
                  </div>
                </div>

              </div>

            </div>
          {/each}
        </div>
      {/if}
    </div>

  {/if}

  <!-- Video Analytics Modal -->
  {#if selectedVideoId}
    <VideoAnalyticsModal 
      videoId={selectedVideoId} 
      onClose={() => selectedVideoId = null}
    />
  {/if}

  <!-- Add Channel Modal -->
  {#if showAddModal}
    <div class="fixed inset-0 z-50 bg-gray-950/80 backdrop-blur-md flex items-center justify-center p-4">
      <div class="w-full max-w-md bg-gray-900 border border-gray-800 rounded-2xl shadow-2xl p-6 space-y-5 animate-scaleUp">
        
        <div class="flex items-center justify-between border-b border-gray-800 pb-3">
          <div class="flex items-center space-x-2">
            <Tv class="w-5 h-5 text-indigo-400" />
            <h3 class="text-base font-bold text-white">Add Your YouTube Channel</h3>
          </div>
          <button 
            type="button" 
            onclick={() => showAddModal = false}
            class="p-1 text-gray-400 hover:text-white rounded-lg hover:bg-gray-800 transition-colors cursor-pointer"
          >
            ✕
          </button>
        </div>

        <form onsubmit={(e) => { e.preventDefault(); handleAddChannel(); }} class="space-y-4">
          
          <div class="space-y-1.5">
            <label for="chan-input" class="text-xs font-semibold text-gray-300 block">
              YouTube URL, @Handle, or Channel ID *
            </label>
            <input 
              id="chan-input"
              type="text" 
              bind:value={addChannelInput}
              placeholder="e.g. https://www.youtube.com/@BookVerse_channel or @GauravWankhede-TECHVERSE"
              required
              class="w-full px-3.5 py-2.5 rounded-xl bg-gray-950 border border-gray-800 text-sm text-gray-100 placeholder-gray-500 focus:outline-none focus:border-indigo-500"
            />
            <span class="text-[10px] text-gray-500 block">
              Zero API key required. Resolves metadata and ingests videos automatically.
            </span>
          </div>

          <div class="space-y-1.5">
            <label for="chan-name" class="text-xs font-semibold text-gray-300 block">
              Custom Channel Label (Optional)
            </label>
            <input 
              id="chan-name"
              type="text" 
              bind:value={addChannelName}
              placeholder="e.g. BookVerse Main Channel"
              class="w-full px-3.5 py-2.5 rounded-xl bg-gray-950 border border-gray-800 text-sm text-gray-100 placeholder-gray-500 focus:outline-none focus:border-indigo-500"
            />
          </div>

          {#if addError}
            <div class="p-3 rounded-xl bg-rose-500/10 border border-rose-500/20 text-rose-400 text-xs flex items-center space-x-2">
              <AlertTriangle class="w-4 h-4 shrink-0" />
              <span>{addError}</span>
            </div>
          {/if}

          {#if addSuccess}
            <div class="p-3 rounded-xl bg-emerald-500/10 border border-emerald-500/20 text-emerald-400 text-xs flex items-center space-x-2">
              <Check class="w-4 h-4 shrink-0" />
              <span>{addSuccess}</span>
            </div>
          {/if}

          <div class="flex justify-end space-x-3 pt-2">
            <button 
              type="button" 
              onclick={() => showAddModal = false}
              class="px-4 py-2 rounded-xl text-xs font-semibold text-gray-400 hover:text-white hover:bg-gray-800 transition-colors cursor-pointer"
            >
              Cancel
            </button>

            <button 
              type="submit" 
              disabled={addingChannel || !addChannelInput.trim()}
              class="px-5 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 disabled:opacity-50 text-white text-xs font-bold shadow-lg shadow-indigo-500/20 transition-all cursor-pointer flex items-center space-x-2"
            >
              {#if addingChannel}
                <span class="animate-spin w-3.5 h-3.5 border-2 border-white border-t-transparent rounded-full"></span>
                <span>Resolving Channel...</span>
              {:else}
                <Plus class="w-4 h-4" />
                <span>Add Channel</span>
              {/if}
            </button>
          </div>

        </form>

      </div>
    </div>
  {/if}

</div>
