<script lang="ts">
  import { syncManager } from '../lib/syncState.svelte';
  import { 
    RefreshCw, 
    CheckCircle2, 
    Tag, 
    Video, 
    X, 
    ChevronDown, 
    ChevronUp 
  } from 'lucide-svelte';

  let dismissed = $state(false);
  let isMinimized = $state(false);

  const status = $derived(syncManager.status);
  const percent = $derived(
    status.total > 0 ? Math.min(Math.round((status.processed / status.total) * 100), 100) : 0
  );

  // If a new sync starts, ensure card becomes visible again
  $effect(() => {
    if (status.is_running) {
      dismissed = false;
    }
  });
</script>

{#if !dismissed && (status.is_running || (status.finished_at && status.processed > 0))}
  <div class="fixed bottom-5 right-5 z-50 w-88 sm:w-96 rounded-2xl bg-gray-900/95 border border-indigo-500/40 backdrop-blur-xl shadow-2xl p-4 text-gray-100 transition-all duration-300 animate-slideUp select-none">
    
    <!-- Card Header -->
    <div class="flex items-center justify-between border-b border-gray-800/80 pb-2.5">
      <div class="flex items-center space-x-2">
        {#if status.is_running}
          <RefreshCw class="w-4 h-4 text-indigo-400 animate-spin" />
          <span class="text-xs font-bold text-white tracking-wide">
            Live Metadata Background Sync
          </span>
        {:else}
          <CheckCircle2 class="w-4 h-4 text-emerald-400" />
          <span class="text-xs font-bold text-emerald-300 tracking-wide">
            Background Sync Completed
          </span>
        {/if}
      </div>

      <div class="flex items-center space-x-1">
        <button
          type="button"
          onclick={() => isMinimized = !isMinimized}
          class="p-1 rounded-lg text-gray-400 hover:text-white hover:bg-gray-800 transition-colors cursor-pointer"
          title={isMinimized ? "Expand" : "Minimize"}
        >
          {#if isMinimized}
            <ChevronUp class="w-3.5 h-3.5" />
          {:else}
            <ChevronDown class="w-3.5 h-3.5" />
          {/if}
        </button>

        <button
          type="button"
          onclick={() => dismissed = true}
          class="p-1 rounded-lg text-gray-400 hover:text-white hover:bg-gray-800 transition-colors cursor-pointer"
          title="Dismiss card"
        >
          <X class="w-3.5 h-3.5" />
        </button>
      </div>
    </div>

    <!-- Body Content -->
    {#if !isMinimized}
      <div class="mt-3 space-y-3">
        
        <!-- Progress Bar & Percentage -->
        <div>
          <div class="flex justify-between items-center text-xs mb-1.5 font-mono">
            <span class="text-gray-400">
              {status.is_running ? 'Processing Queue' : 'All Videos Processed'}
            </span>
            <span class="font-bold text-indigo-400">
              {status.processed} / {status.total} ({percent}%)
            </span>
          </div>
          <div class="h-2 w-full bg-gray-800 rounded-full overflow-hidden">
            <div 
              class="h-full {status.is_running ? 'bg-gradient-to-r from-indigo-500 to-purple-500' : 'bg-emerald-500'} rounded-full transition-all duration-300"
              style={`width: ${percent}%`}
            ></div>
          </div>
        </div>

        <!-- Live Ticker of Current Video -->
        {#if status.is_running && status.current_title}
          <div class="p-2.5 rounded-xl bg-gray-950/80 border border-gray-800/80 space-y-1">
            <span class="text-[10px] font-mono text-gray-500 uppercase tracking-wider block">
              Currently Syncing
            </span>
            <p class="text-xs text-gray-200 truncate font-medium">
              {status.current_title}
            </p>
          </div>
        {/if}

        <!-- Metrics Grid -->
        <div class="grid grid-cols-2 gap-2 pt-1">
          <div class="p-2 rounded-lg bg-gray-950/60 border border-gray-800 flex items-center space-x-2">
            <Video class="w-3.5 h-3.5 text-indigo-400 shrink-0" />
            <div>
              <span class="text-[10px] text-gray-400 block leading-none">Synced</span>
              <span class="text-xs font-bold font-mono text-white">{status.processed} vids</span>
            </div>
          </div>

          <div class="p-2 rounded-lg bg-gray-950/60 border border-gray-800 flex items-center space-x-2">
            <Tag class="w-3.5 h-3.5 text-emerald-400 shrink-0" />
            <div>
              <span class="text-[10px] text-gray-400 block leading-none">Tags Extracted</span>
              <span class="text-xs font-bold font-mono text-emerald-400">+{status.tags_synced} tags</span>
            </div>
          </div>
        </div>

      </div>
    {/if}

  </div>
{/if}
