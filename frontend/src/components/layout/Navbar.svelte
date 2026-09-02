<script lang="ts">
  import { rpc } from '../../lib/rpc.svelte';
  import { 
    LayoutDashboard, 
    Search, 
    Kanban, 
    Sparkles, 
    Radio, 
    Image, 
    Activity, 
    Flame,
    Cpu
  } from 'lucide-svelte';

  let { currentRoute = $bindable('dashboard') }: { currentRoute: string } = $props();

  const navItems = [
    { id: 'dashboard', label: 'Cockpit', icon: LayoutDashboard },
    { id: 'research', label: 'Research', icon: Search },
    { id: 'kanban', label: 'Production Kanban', icon: Kanban },
    { id: 'teleprompter', label: 'Script Studio', icon: Radio },
    { id: 'thumbnail', label: 'Thumbnail Studio', icon: Image },
    { id: 'gaps', label: 'Outlier Gaps', icon: Flame },
    { id: 'scores', label: 'SEO & GEO Scores', icon: Sparkles },
    { id: 'health', label: 'System Health', icon: Activity },
  ];
</script>

<header class="border-b border-gray-800 bg-gray-950/80 backdrop-blur-md sticky top-0 z-50">
  <div class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
    <div class="flex items-center justify-between h-16">
      
      <!-- Brand Lockup -->
      <button 
        type="button"
        class="flex items-center space-x-3 text-left focus:outline-none cursor-pointer" 
        onclick={() => currentRoute = 'dashboard'}
      >
        <div class="w-9 h-9 rounded-xl bg-gradient-to-br from-indigo-500 to-purple-600 flex items-center justify-center shadow-lg shadow-indigo-500/20">
          <Cpu class="w-5 h-5 text-white" />
        </div>
        <div>
          <span class="text-lg font-bold tracking-tight bg-gradient-to-r from-white via-gray-200 to-gray-400 bg-clip-text text-transparent">
            TubeForge
          </span>
          <span class="ml-2 text-[10px] font-mono uppercase px-2 py-0.5 rounded-full bg-indigo-500/10 text-indigo-400 border border-indigo-500/20">
            v0.3.0 Cockpit
          </span>
        </div>
      </button>

      <!-- Navigation Tabs -->
      <nav class="hidden md:flex space-x-1">
        {#each navItems as item}
          {@const Icon = item.icon}
          <button
            type="button"
            onclick={() => currentRoute = item.id}
            class="flex items-center space-x-2 px-3.5 py-2 rounded-lg text-xs font-medium transition-all duration-150 cursor-pointer {
              currentRoute === item.id 
                ? 'bg-indigo-500/15 text-indigo-400 border border-indigo-500/30 shadow-sm' 
                : 'text-gray-400 hover:text-gray-200 hover:bg-gray-900/60'
            }"
          >
            <Icon class="w-4 h-4" />
            <span>{item.label}</span>
          </button>
        {/each}
      </nav>

      <!-- RPC Live Status -->
      <div class="flex items-center space-x-3">
        <div class="flex items-center space-x-2 px-2.5 py-1 rounded-full text-xs font-mono {
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
          <span class="capitalize">{rpc.status}</span>
        </div>
      </div>

    </div>
  </div>
</header>
