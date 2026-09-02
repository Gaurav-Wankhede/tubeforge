<script lang="ts">
  import Sidebar from './components/layout/Sidebar.svelte';
  import Dashboard from './routes/Dashboard.svelte';
  import PersonalStudio from './routes/PersonalStudio.svelte';
  import TopicResearch from './routes/TopicResearch.svelte';
  import KanbanBoard from './routes/KanbanBoard.svelte';
  import Teleprompter from './routes/Teleprompter.svelte';
  import ThumbnailStudio from './routes/ThumbnailStudio.svelte';
  import Gaps from './routes/Gaps.svelte';
  import Scores from './routes/Scores.svelte';
  import KeywordsRadar from './routes/KeywordsRadar.svelte';
  import ChannelAudit from './routes/ChannelAudit.svelte';
  import SyncProgressCard from './components/SyncProgressCard.svelte';
  import { Menu, Sparkles, Cpu, Bell, RefreshCw, Check } from 'lucide-svelte';
  import { rpc } from './lib/rpc.svelte';
  import { syncManager } from './lib/syncState.svelte';
  import { onMount } from 'svelte';

  const routeTitles: Record<string, { title: string; subtitle: string }> = {
    dashboard: { title: 'Overview Cockpit', subtitle: 'Real-time telemetry, storage metrics & breakout outlier intelligence' },
    studio: { title: 'Personal Channel Studio', subtitle: 'Isolated channel performance, competitor benchmark gaps & prescriptive playbooks' },
    research: { title: 'Topic & SERP Hunter', subtitle: '5-stage competitive research, BM25 retrieval & Kanban packaging' },
    keywords: { title: 'Keyword Search Radar', subtitle: 'Rank tracking, BM25 corpus resonance & competitor search trends' },
    kanban: { title: 'Production Kanban', subtitle: 'Interactive storyboards, 0:00–0:45 blueprints & 3-witness verification' },
    teleprompter: { title: 'Script Teleprompter', subtitle: 'Native 60fps high-retention script prompter with dynamic WPM velocity' },
    thumbnail: { title: 'Thumbnail Studio', subtitle: '1280x720 canvas editor with high-contrast typography & CTR presets' },
    gaps: { title: 'Outlier & Gap Analysis', subtitle: 'SERP competitive gaps, BM25 semantic clusters & opportunity scores' },
    scores: { title: 'Video Quality Ratings', subtitle: '18 SEO & 7 GEO algorithmic signals with interactive YouTube card grid' },
    audit: { title: 'Channel & System Audit', subtitle: 'Channel trust ratings, growth chronology & SQLite database integrity' },
  };

  function getStoredRoute(): string {
    if (typeof window !== 'undefined') {
      const hash = window.location.hash.replace(/^#\/?/, '');
      if (hash && routeTitles[hash]) return hash;
      const stored = localStorage.getItem('tubeforge_active_route');
      if (stored && routeTitles[stored]) return stored;
    }
    return 'dashboard';
  }

  let currentRoute = $state(getStoredRoute());
  let sidebarOpen = $state(false);
  let initialSearchQuery = $state<string | undefined>(undefined);

  const syncStatus = $derived(syncManager.status);
  const syncPercent = $derived(
    syncStatus.total > 0 ? Math.min(Math.round((syncStatus.processed / syncStatus.total) * 100), 100) : 0
  );

  $effect(() => {
    if (typeof window !== 'undefined') {
      localStorage.setItem('tubeforge_active_route', currentRoute);
      if (window.location.hash !== `#${currentRoute}`) {
        window.location.hash = `#${currentRoute}`;
      }
    }
  });

  function navigate(route: string, param?: string) {
    if (route === 'research' && param) {
      initialSearchQuery = param;
    }
    currentRoute = route;
  }

  onMount(() => {
    const handleHash = () => {
      const hash = window.location.hash.replace(/^#\/?/, '');
      if (hash && routeTitles[hash] && hash !== currentRoute) {
        currentRoute = hash;
      }
    };
    window.addEventListener('hashchange', handleHash);
    return () => {
      window.removeEventListener('hashchange', handleHash);
    };
  });

  function triggerLiveSync() {
    syncManager.triggerSync();
  }

  const currentInfo = $derived(routeTitles[currentRoute] || { title: 'Creator Cockpit', subtitle: 'YouTube Growth Engine' });
</script>

<div class="min-h-screen bg-gray-950 text-gray-100 flex antialiased selection:bg-indigo-500/30 selection:text-indigo-200">
  
  <!-- Responsive Left Sidebar -->
  <Sidebar bind:currentRoute bind:sidebarOpen />

  <!-- Main Viewport Area (Offset for Desktop Sidebar) -->
  <div class="flex-1 flex flex-col min-w-0 lg:pl-64 transition-all duration-300">
    
    <!-- Top Header Bar -->
    <header class="h-16 border-b border-gray-850 bg-gray-950/80 backdrop-blur-md sticky top-0 z-30 flex items-center justify-between px-4 sm:px-6 lg:px-8">
      
      <!-- Left: Mobile Menu Toggle & Breadcrumbs -->
      <div class="flex items-center space-x-3 sm:space-x-4">
        <button 
          type="button"
          onclick={() => sidebarOpen = true}
          class="lg:hidden p-2 rounded-xl bg-gray-900 border border-gray-800 text-gray-300 hover:text-white hover:bg-gray-855 transition-colors cursor-pointer"
          title="Open Navigation"
        >
          <Menu class="w-5 h-5" />
        </button>

        <div>
          <h1 class="text-sm sm:text-base font-bold text-white tracking-tight leading-none">
            {currentInfo.title}
          </h1>
          <p class="hidden sm:block text-[11px] text-gray-400 truncate max-w-md mt-0.5">
            {currentInfo.subtitle}
          </p>
        </div>
      </div>

      <!-- Right: Sync Live Stats & Live Engine Indicator -->
      <div class="flex items-center space-x-3">
        
        <!-- Live YouTube Metadata Sync Button with Live Progress % -->
        <button
          type="button"
          onclick={triggerLiveSync}
          disabled={syncStatus.is_running}
          class="inline-flex items-center space-x-2 px-3.5 py-1.5 rounded-xl text-xs font-bold transition-all cursor-pointer relative overflow-hidden {
            syncStatus.is_running
              ? 'bg-indigo-950 border border-indigo-500/40 text-indigo-300 shadow-lg shadow-indigo-500/20'
              : 'bg-gray-900 hover:bg-gray-850 text-gray-300 hover:text-white border border-gray-800 hover:border-indigo-500/30'
          }"
          title="Fetch latest live views, likes & tags from YouTube for all videos"
        >
          <RefreshCw class="w-3.5 h-3.5 {syncStatus.is_running ? 'animate-spin text-indigo-400' : 'text-gray-400'}" />
          <span>
            {syncStatus.is_running ? `Syncing (${syncPercent}%)` : 'Sync Live Stats'}
          </span>
          {#if syncStatus.is_running}
            <div 
              class="absolute bottom-0 left-0 h-0.5 bg-gradient-to-r from-indigo-500 to-purple-500 transition-all duration-300"
              style={`width: ${syncPercent}%`}
            ></div>
          {/if}
        </button>

        <div class="flex items-center space-x-2 px-3 py-1 rounded-full text-xs font-mono {
          rpc.status === 'connected' 
            ? 'bg-emerald-500/10 text-emerald-400 border border-emerald-500/20' 
            : rpc.status === 'connecting'
            ? 'bg-amber-500/10 text-amber-400 border border-amber-500/20'
            : 'bg-rose-500/10 text-rose-400 border border-rose-500/20'
        }">
          <span class="relative flex h-2 w-2">
            {#if rpc.status === 'connected'}
              <span class="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75"></span>
            {/if}
            <span class="relative inline-flex rounded-full h-2 w-2 {
              rpc.status === 'connected' ? 'bg-emerald-500' : rpc.status === 'connecting' ? 'bg-amber-500' : 'bg-rose-500'
            }"></span>
          </span>
          <span class="hidden sm:inline font-semibold">Engine</span>
          <span class="capitalize">{rpc.status}</span>
        </div>
      </div>

    </header>

    <!-- Main Content Body -->
    <main class="flex-1 w-full max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-6 sm:py-8">
      {#if currentRoute === 'dashboard'}
        <Dashboard onNavigate={navigate} />
      {:else if currentRoute === 'studio'}
        <PersonalStudio onNavigate={navigate} />
      {:else if currentRoute === 'research'}
        <TopicResearch initialQuery={initialSearchQuery} onNavigate={navigate} />
      {:else if currentRoute === 'keywords'}
        <KeywordsRadar onNavigate={navigate} />
      {:else if currentRoute === 'kanban'}
        <KanbanBoard />
      {:else if currentRoute === 'teleprompter'}
        <Teleprompter />
      {:else if currentRoute === 'thumbnail'}
        <ThumbnailStudio />
      {:else if currentRoute === 'gaps'}
        <Gaps />
      {:else if currentRoute === 'scores'}
        <Scores onNavigate={navigate} />
      {:else if currentRoute === 'audit'}
        <ChannelAudit />
      {/if}
    </main>

    <!-- Footer -->
    <footer class="border-t border-gray-900 py-6 text-center text-xs text-gray-600 font-mono">
      TubeForge Real-Time Creator Cockpit · Local-First Architecture · Port 17487
    </footer>

  </div>

  <!-- Live Background Sync Progress Card -->
  <SyncProgressCard />

</div>
