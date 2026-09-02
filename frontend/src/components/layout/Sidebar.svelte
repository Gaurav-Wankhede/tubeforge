<script lang="ts">
  import { rpc } from '../../lib/rpc.svelte';
  import { 
    LayoutDashboard, 
    Search, 
    Kanban, 
    Sparkles, 
    Radio, 
    Image, 
    Flame,
    Cpu,
    X,
    Database,
    KeyRound,
    ShieldCheck,
    RefreshCw,
    CheckCircle2,
    Tag,
    Tv
  } from 'lucide-svelte';
  import { syncManager } from '../../lib/syncState.svelte';

  let { 
    currentRoute = $bindable('dashboard'),
    sidebarOpen = $bindable(false)
  }: { 
    currentRoute: string;
    sidebarOpen?: boolean;
  } = $props();

  const syncStatus = $derived(syncManager.status);
  const syncPct = $derived(
    syncStatus.total > 0 ? Math.min(Math.round((syncStatus.processed / syncStatus.total) * 100), 100) : 0
  );

  function triggerSync() {
    syncManager.triggerSync();
  }

  const navSections = [
    {
      title: 'Growth & Intelligence',
      items: [
        { id: 'dashboard', label: 'Overview Cockpit', icon: LayoutDashboard, badge: null },
        { id: 'studio', label: 'Personal Studio', icon: Tv, badge: 'My Channels' },
        { id: 'research', label: 'Topic & SERP Hunter', icon: Search, badge: '5-Stage' },
        { id: 'keywords', label: 'Keyword Search Radar', icon: KeyRound, badge: 'SERP' },
        { id: 'gaps', label: 'Outlier & Gap Analysis', icon: Flame, badge: 'BM25' },
        { id: 'scores', label: 'Video Quality Ratings', icon: Sparkles, badge: '18 SEO' },
      ]
    },
    {
      title: 'Content Production',
      items: [
        { id: 'kanban', label: 'Production Kanban', icon: Kanban, badge: null },
        { id: 'teleprompter', label: 'Script Teleprompter', icon: Radio, badge: '60 FPS' },
        { id: 'thumbnail', label: 'Thumbnail Studio', icon: Image, badge: '1280x720' },
        { id: 'audit', label: 'Channel & System Audit', icon: ShieldCheck, badge: 'Health' },
      ]
    }
  ];

  function handleSelect(id: string) {
    currentRoute = id;
    sidebarOpen = false;
  }
</script>

<!-- Mobile Backdrop Overlay -->
{#if sidebarOpen}
  <div 
    class="fixed inset-0 z-40 bg-gray-950/80 backdrop-blur-sm lg:hidden transition-opacity duration-300"
    role="button"
    tabindex="0"
    onclick={() => sidebarOpen = false}
    onkeydown={(e) => { if (e.key === 'Escape') sidebarOpen = false; }}
  ></div>
{/if}

<!-- Sidebar Container -->
<aside 
  class="fixed top-0 bottom-0 left-0 z-50 w-64 bg-gray-950/95 border-r border-gray-800 flex flex-col justify-between transition-transform duration-300 ease-in-out lg:translate-x-0 {
    sidebarOpen ? 'translate-x-0 shadow-2xl' : '-translate-x-full'
  }"
>
  <!-- Top Branding -->
  <div>
    <div class="h-16 flex items-center justify-between px-5 border-b border-gray-800/80">
      <button 
        type="button"
        class="flex items-center space-x-3 text-left focus:outline-none cursor-pointer group" 
        onclick={() => handleSelect('dashboard')}
      >
        <div class="w-9 h-9 rounded-xl bg-gradient-to-br from-indigo-500 via-indigo-600 to-purple-600 flex items-center justify-center shadow-lg shadow-indigo-500/25 group-hover:scale-105 transition-transform">
          <Cpu class="w-5 h-5 text-white" />
        </div>
        <div>
          <div class="flex items-center space-x-1.5">
            <span class="text-base font-extrabold tracking-tight bg-gradient-to-r from-white via-gray-100 to-gray-300 bg-clip-text text-transparent">
              TubeForge
            </span>
          </div>
          <span class="text-[10px] font-mono text-indigo-400/90 font-medium">
            Creator Intelligence
          </span>
        </div>
      </button>

      <!-- Mobile Close Button -->
      <button 
        type="button"
        onclick={() => sidebarOpen = false}
        class="lg:hidden p-1.5 rounded-lg text-gray-400 hover:text-white hover:bg-gray-800 transition-colors cursor-pointer"
        title="Close Sidebar"
      >
        <X class="w-5 h-5" />
      </button>
    </div>

    <!-- Navigation Link Groups -->
    <div class="py-5 px-3 space-y-6 overflow-y-auto max-h-[calc(100vh-180px)]">
      {#each navSections as section}
        <div class="space-y-1">
          <div class="px-3 pb-1.5 text-[10px] font-mono uppercase tracking-wider text-gray-500 font-bold">
            {section.title}
          </div>

          <div class="space-y-0.5">
            {#each section.items as item}
              {@const Icon = item.icon}
              {@const isActive = currentRoute === item.id}
              
              <button
                type="button"
                onclick={() => handleSelect(item.id)}
                class="w-full flex items-center justify-between px-3 py-2.5 rounded-xl text-xs font-medium transition-all duration-150 cursor-pointer group {
                  isActive 
                    ? 'bg-gradient-to-r from-indigo-600/20 to-purple-600/10 text-white font-semibold border border-indigo-500/30 shadow-md shadow-indigo-500/5' 
                    : 'text-gray-400 hover:text-gray-100 hover:bg-gray-900/60 border border-transparent'
                }"
              >
                <div class="flex items-center space-x-3 truncate">
                  <div class="p-1 rounded-lg transition-colors {
                    isActive 
                      ? 'bg-indigo-600 text-white shadow-sm' 
                      : 'text-gray-400 group-hover:text-indigo-400 group-hover:bg-gray-800/80'
                  }">
                    <Icon class="w-4 h-4" />
                  </div>
                  <span class="truncate">{item.label}</span>
                </div>

                {#if item.badge}
                  <span class="text-[9px] font-mono px-1.5 py-0.5 rounded-md {
                    isActive 
                      ? 'bg-indigo-500/30 text-indigo-300 font-bold' 
                      : 'bg-gray-800/80 text-gray-500 group-hover:text-gray-400'
                  }">
                    {item.badge}
                  </span>
                {/if}
              </button>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  </div>

  <!-- Bottom System Status & Telemetry -->
  <div class="p-3 border-t border-gray-800/80 bg-gray-950/60 space-y-2">
    
    <!-- Background Task & Sync Status Widget -->
    <div class="p-2.5 rounded-xl bg-gray-900/90 border {syncStatus.is_running ? 'border-indigo-500/50 shadow-md shadow-indigo-500/10' : 'border-gray-800'} space-y-2">
      <div class="flex items-center justify-between">
        <div class="flex items-center space-x-2">
          {#if syncStatus.is_running}
            <RefreshCw class="w-3.5 h-3.5 text-indigo-400 animate-spin" />
            <span class="text-[11px] font-bold text-indigo-300">Syncing YouTube...</span>
          {:else if syncStatus.finished_at && syncStatus.processed > 0}
            <CheckCircle2 class="w-3.5 h-3.5 text-emerald-400" />
            <span class="text-[11px] font-bold text-emerald-400">Sync Complete</span>
          {:else}
            <RefreshCw class="w-3.5 h-3.5 text-gray-500" />
            <span class="text-[11px] font-bold text-gray-300">Live Metadata Sync</span>
          {/if}
        </div>

        <button
          type="button"
          onclick={triggerSync}
          disabled={syncStatus.is_running}
          class="text-[10px] font-mono px-2 py-0.5 rounded-md bg-indigo-600/30 hover:bg-indigo-600/50 text-indigo-300 border border-indigo-500/30 transition-colors cursor-pointer disabled:opacity-50"
          title="Trigger live background sync"
        >
          {syncStatus.is_running ? `${syncPct}%` : 'Sync'}
        </button>
      </div>

      {#if syncStatus.is_running}
        <div class="space-y-1">
          <div class="h-1.5 w-full bg-gray-950 rounded-full overflow-hidden">
            <div 
              class="h-full bg-gradient-to-r from-indigo-500 to-purple-500 rounded-full transition-all duration-300"
              style={`width: ${syncPct}%`}
            ></div>
          </div>
          <div class="flex justify-between text-[9px] font-mono text-gray-400">
            <span>{syncStatus.processed} / {syncStatus.total}</span>
            <span>+{syncStatus.tags_synced} tags</span>
          </div>
        </div>
      {:else if syncStatus.finished_at}
        <div class="text-[9px] font-mono text-gray-500 truncate">
          {syncStatus.processed} videos · +{syncStatus.tags_synced} tags
        </div>
      {/if}
    </div>

    <!-- WebSocket Connection Status -->
    <div class="p-2 rounded-xl bg-gray-900/60 border border-gray-850 flex items-center justify-between">
      <div class="flex items-center space-x-2">
        <span class="relative flex h-2 w-2">
          {#if rpc.status === 'connected'}
            <span class="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75"></span>
          {/if}
          <span class="relative inline-flex rounded-full h-2 w-2 {
            rpc.status === 'connected' ? 'bg-emerald-500' : rpc.status === 'connecting' ? 'bg-amber-500' : 'bg-rose-500'
          }"></span>
        </span>
        <span class="text-[11px] font-medium text-gray-300 capitalize">
          {rpc.status === 'connected' ? 'Engine Live' : rpc.status}
        </span>
      </div>

      <span class="text-[10px] font-mono text-gray-500">
        :17487
      </span>
    </div>

    <!-- Storage Badge -->
    <div class="px-2 py-1 rounded-lg bg-gray-900/40 flex items-center justify-between text-[10px] font-mono text-gray-400 border border-gray-800/40">
      <div class="flex items-center space-x-1.5">
        <Database class="w-3 h-3 text-indigo-400" />
        <span>SQLite WAL</span>
      </div>
      <span class="text-emerald-400 font-semibold">Zero-Mock</span>
    </div>

  </div>
</aside>
