<script lang="ts">
  import { onMount } from 'svelte';
  import { rpc } from '../lib/rpc.svelte';
  import type { Counts, KanbanSummary, ScoreRow } from '../lib/types';
  import { 
    Video, 
    Tv, 
    Tag, 
    Lightbulb, 
    Kanban, 
    ArrowRight, 
    Sparkles, 
    Activity, 
    Zap,
    TrendingUp,
    Flame,
    Plus,
    RefreshCw,
    CheckCircle2
  } from 'lucide-svelte';
  import MediaCard from '../components/MediaCard.svelte';
  import VideoAnalyticsModal from '../components/VideoAnalyticsModal.svelte';
  import { syncManager } from '../lib/syncState.svelte';

  let counts = $state<Counts | null>(null);
  let kanbanSummary = $state<KanbanSummary | null>(null);
  let topVideos = $state<ScoreRow[]>([]);
  let nextVideoRec = $state<any>(null);
  let selectedVideoId = $state<string | null>(null);
  let loading = $state(true);

  const syncStatus = $derived(syncManager.status);

  let { onNavigate }: { onNavigate: (route: string) => void } = $props();

  async function loadData() {
    loading = true;
    try {
      const countsRes = await fetch('/api/counts');
      if (countsRes.ok) {
        counts = await countsRes.json();
      }

      const kanbanRes = await rpc.call('kanban.list', {});
      if (kanbanRes && kanbanRes.summary) {
        kanbanSummary = kanbanRes.summary;
      }

      const nextVideoRes = await fetch('/api/analysis/next-video');
      if (nextVideoRes.ok) {
        const nextData = await nextVideoRes.json();
        nextVideoRec = nextData.recommendation;
      }

      const scoresRes = await fetch('/api/scores');
      if (scoresRes.ok) {
        const allScores: ScoreRow[] = await scoresRes.json();
        // Top 8 videos sorted by breakout multiplier / views
        topVideos = allScores
          .sort((a, b) => ((b.outlier_multiplier || 1) * (b.overall_score || 1)) - ((a.outlier_multiplier || 1) * (a.overall_score || 1)))
          .slice(0, 8);
      }
    } catch {
      // ignore
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    loadData();
  });
</script>

<div class="space-y-8">
  
  <!-- Hero Banner -->
  <div class="relative overflow-hidden rounded-2xl bg-gradient-to-r from-gray-900 via-indigo-950/40 to-gray-900 border border-gray-800 p-6 sm:p-8">
    <div class="relative z-10 max-w-3xl space-y-3">
      <div class="inline-flex items-center space-x-2 px-3 py-1 rounded-full bg-indigo-500/10 border border-indigo-500/20 text-indigo-400 text-xs font-semibold tracking-wide uppercase">
        <Zap class="w-3.5 h-3.5" />
        <span>Unified Creator Cockpit</span>
      </div>
      <h1 class="text-2xl sm:text-3xl font-extrabold tracking-tight text-white">
        YouTube SEO & GEO Intelligence
      </h1>
      <p class="text-sm sm:text-base text-gray-300 leading-relaxed">
        Autonomous keyword analytics, BM25 semantic retrieval, 18 SEO and 7 GEO algorithmic signals, interconnected production Kanban, and 60fps script teleprompter.
      </p>
      
      <div class="pt-2 flex flex-wrap gap-3">
        <button
          onclick={() => onNavigate('research')}
          class="inline-flex items-center space-x-2 px-4 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-bold shadow-lg shadow-indigo-500/20 transition-all cursor-pointer"
        >
          <Sparkles class="w-4 h-4" />
          <span>Launch Topic Research</span>
          <ArrowRight class="w-3.5 h-3.5" />
        </button>

        <button
          onclick={() => onNavigate('kanban')}
          class="inline-flex items-center space-x-2 px-4 py-2 rounded-xl bg-gray-900 hover:bg-gray-800 border border-gray-700 text-gray-200 text-xs font-bold transition-all cursor-pointer"
        >
          <Kanban class="w-4 h-4 text-indigo-400" />
          <span>Production Kanban ({kanbanSummary?.total || 0})</span>
        </button>
      </div>
    </div>
  </div>

  <!-- Autonomous Next Best Video Recommendation Card (When Available) -->
  {#if nextVideoRec && nextVideoRec.topic}
    <div class="relative overflow-hidden rounded-2xl bg-gradient-to-r from-amber-950/30 via-gray-900 to-indigo-950/30 border border-amber-500/30 p-5 sm:p-6 shadow-xl">
      <div class="flex flex-col md:flex-row md:items-center justify-between gap-4">
        <div class="space-y-1.5 max-w-2xl">
          <div class="flex items-center space-x-2">
            <span class="inline-flex items-center space-x-1 px-2.5 py-0.5 rounded-full bg-amber-500/10 border border-amber-500/20 text-amber-400 font-mono text-[11px] font-bold">
              <Flame class="w-3.5 h-3.5 text-amber-400" />
              <span>AUTONOMOUS NEXT BEST VIDEO RECOMMENDATION</span>
            </span>
            <span class="text-[11px] font-mono px-2 py-0.5 rounded bg-gray-800 text-gray-300">
              {nextVideoRec.volume_label} Volume
            </span>
          </div>
          <h2 class="text-base sm:text-lg font-black text-white leading-snug">
            {nextVideoRec.title?.replace(/:/g, ' — ') || nextVideoRec.topic}
          </h2>
          <p class="text-xs text-gray-400 leading-relaxed">
            {nextVideoRec.why}
          </p>
        </div>

        <div class="flex items-center space-x-3 shrink-0">
          <button
            type="button"
            onclick={async () => {
              const cleanTitle = (nextVideoRec.title || nextVideoRec.topic).replace(/:/g, ' — ');
              await rpc.call('kanban.create', {
                title: cleanTitle,
                topic: nextVideoRec.topic,
                channel: 'TECHVERSE',
                framework: 'Autonomous Growth Recommendation',
                optimal_duration_sec: 720,
                status: 'todo',
                notes: nextVideoRec.description || '',
              });
              onNavigate('kanban');
            }}
            class="inline-flex items-center space-x-2 px-4 py-2.5 rounded-xl bg-amber-500 hover:bg-amber-400 text-gray-950 font-bold text-xs shadow-lg shadow-amber-500/20 transition-all cursor-pointer"
          >
            <Plus class="w-4 h-4" />
            <span>Add to Kanban Pipeline</span>
          </button>
        </div>
      </div>
    </div>
  {/if}

  <!-- Live Background Sync Monitor Card (Always Visible when running or synced) -->
  {#if syncStatus && (syncStatus.is_running || syncStatus.processed > 0)}
    {@const pct = syncStatus.total > 0 ? Math.min(Math.round((syncStatus.processed / syncStatus.total) * 100), 100) : 0}
    <div class="p-5 rounded-2xl bg-gray-900/80 border {syncStatus.is_running ? 'border-indigo-500/50 shadow-lg shadow-indigo-500/10' : 'border-emerald-500/40'} space-y-3">
      <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-2">
        <div class="flex items-center space-x-2">
          {#if syncStatus.is_running}
            <RefreshCw class="w-4 h-4 text-indigo-400 animate-spin" />
            <span class="text-xs font-bold text-white uppercase tracking-wider">
              Background Synchronization In Progress
            </span>
          {:else}
            <CheckCircle2 class="w-4 h-4 text-emerald-400" />
            <span class="text-xs font-bold text-emerald-300 uppercase tracking-wider">
              Live YouTube Metadata Synchronization Complete
            </span>
          {/if}
          <span class="text-[11px] font-mono px-2 py-0.5 rounded bg-gray-800 text-gray-300">
            {syncStatus.processed} / {syncStatus.total} ({pct}%)
          </span>
        </div>

        {#if syncStatus.is_running && syncStatus.current_title}
          <span class="text-xs text-gray-400 truncate max-w-sm font-mono">
            Syncing: <span class="text-gray-200">{syncStatus.current_title}</span>
          </span>
        {/if}
      </div>

      <!-- Animated Progress Bar -->
      <div class="h-2 w-full bg-gray-950 rounded-full overflow-hidden border border-gray-800">
        <div 
          class="h-full {syncStatus.is_running ? 'bg-gradient-to-r from-indigo-500 via-purple-500 to-indigo-500 animate-pulse' : 'bg-emerald-500'} rounded-full transition-all duration-300"
          style={`width: ${pct}%`}
        ></div>
      </div>

      <div class="flex flex-wrap items-center justify-between text-xs text-gray-400 font-mono pt-1">
        <span>{syncStatus.message}</span>
        {#if syncStatus.finished_at}
          <span class="text-gray-500">Finished: {new Date(syncStatus.finished_at).toLocaleTimeString()}</span>
        {/if}
      </div>
    </div>
  {/if}

  <!-- Key Metrics Row -->
  <div class="grid grid-cols-2 lg:grid-cols-4 gap-4">
    
    <div class="p-5 rounded-xl bg-gray-900/60 border border-gray-800 flex items-center justify-between">
      <div>
        <p class="text-xs font-medium text-gray-400">Indexed Videos</p>
        <p class="mt-1 text-2xl font-extrabold text-white font-mono">
          {counts?.videos || 0}
        </p>
        <p class="mt-1 text-[11px] text-gray-500">Pure SQLite WAL Storage</p>
      </div>
      <div class="w-10 h-10 rounded-xl bg-indigo-500/10 border border-indigo-500/20 flex items-center justify-center text-indigo-400">
        <Video class="w-5 h-5" />
      </div>
    </div>

    <div class="p-5 rounded-xl bg-gray-900/60 border border-gray-800 flex items-center justify-between">
      <div>
        <p class="text-xs font-medium text-gray-400">Tracked Channels</p>
        <p class="mt-1 text-2xl font-extrabold text-white font-mono">
          {counts?.channels || 0}
        </p>
        <p class="mt-1 text-[11px] text-gray-500">Competitors & Ingestion</p>
      </div>
      <div class="w-10 h-10 rounded-xl bg-purple-500/10 border border-purple-500/20 flex items-center justify-center text-purple-400">
        <Tv class="w-5 h-5" />
      </div>
    </div>

    <div class="p-5 rounded-xl bg-gray-900/60 border border-gray-800 flex items-center justify-between">
      <div>
        <p class="text-xs font-medium text-gray-400">Production Tickets</p>
        <p class="mt-1 text-2xl font-extrabold text-emerald-400 font-mono">
          {kanbanSummary?.total || 0}
        </p>
        <p class="mt-1 text-[11px] text-gray-500">
          {kanbanSummary?.todo || 0} To-Do · {kanbanSummary?.inprogress || 0} In-Progress
        </p>
      </div>
      <div class="w-10 h-10 rounded-xl bg-emerald-500/10 border border-emerald-500/20 flex items-center justify-center text-emerald-400">
        <Kanban class="w-5 h-5" />
      </div>
    </div>

    <div class="p-5 rounded-xl bg-gray-900/60 border border-gray-800 flex items-center justify-between">
      <div>
        <p class="text-xs font-medium text-gray-400">Tracked Keywords</p>
        <p class="mt-1 text-2xl font-extrabold text-indigo-400 font-mono">
          {counts?.keywords || 0}
        </p>
        <p class="mt-1 text-[11px] text-gray-500">SERP Snapshot History</p>
      </div>
      <div class="w-10 h-10 rounded-xl bg-indigo-500/10 border border-indigo-500/20 flex items-center justify-center text-indigo-400">
        <Tag class="w-5 h-5" />
      </div>
    </div>

  </div>

  <!-- Top Breakout YouTube Video Cards Section -->
  {#if topVideos.length > 0}
    <div class="space-y-4">
      <div class="flex items-center justify-between">
        <div class="flex items-center space-x-2">
          <Flame class="w-5 h-5 text-amber-400" />
          <h2 class="text-lg font-bold text-white tracking-tight">
            Breakthrough & High-Performing Videos
          </h2>
        </div>
        <button
          onclick={() => onNavigate('scores')}
          class="text-xs text-indigo-400 hover:text-indigo-300 font-medium inline-flex items-center space-x-1 cursor-pointer"
        >
          <span>View All ({counts?.videos || 0})</span>
          <ArrowRight class="w-3.5 h-3.5" />
        </button>
      </div>

      <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-5">
        {#each topVideos as video}
          <MediaCard 
            {video} 
            onSelect={() => selectedVideoId = video.video_id}
            onInspectGaps={() => onNavigate('gaps')}
          />
        {/each}
      </div>
    </div>
  {/if}

  <!-- Real-Time Event Stream & Status Ticker -->
  <div class="rounded-xl bg-gray-900/50 border border-gray-800 p-5">
    <div class="flex items-center justify-between mb-4">
      <div class="flex items-center space-x-2">
        <Activity class="w-4 h-4 text-emerald-400 animate-pulse" />
        <h2 class="text-sm font-bold text-gray-200 uppercase tracking-wider">
          Real-Time Background Activity Stream
        </h2>
      </div>
      <span class="text-xs text-gray-500 font-mono">
        Duplex WebSocket /ws
      </span>
    </div>

    {#if rpc.recentEvents.length === 0}
      <div class="p-6 text-center text-xs text-gray-500 bg-gray-950/40 rounded-lg border border-gray-800/60">
        Waiting for background topic hunt or ingestion events. Run a research query or CLI command to see live telemetry.
      </div>
    {:else}
      <div class="space-y-2 max-h-48 overflow-y-auto pr-1">
        {#each rpc.recentEvents as ev}
          <div class="px-3 py-2 rounded-lg bg-gray-950/80 border border-gray-800 flex items-center justify-between text-xs">
            <div class="flex items-center space-x-2">
              <span class="w-2 h-2 rounded-full bg-indigo-400"></span>
              <span class="font-mono text-indigo-300 font-semibold">{ev.event}</span>
              <span class="text-gray-400">{JSON.stringify(ev.data)}</span>
            </div>
            <span class="text-[11px] text-gray-500 font-mono">{ev.timestamp}</span>
          </div>
        {/each}
      </div>
    {/if}
  </div>

  <!-- Video Analytics Modal -->
  {#if selectedVideoId}
    <VideoAnalyticsModal 
      videoId={selectedVideoId} 
      onClose={() => selectedVideoId = null}
    />
  {/if}

</div>

