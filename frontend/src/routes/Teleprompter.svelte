<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { 
    Play, 
    Pause, 
    RotateCcw, 
    Maximize2, 
    Minimize2, 
    Type, 
    Gauge, 
    Clock, 
    Sparkles, 
    FileText,
    Kanban,
    Save,
    Check 
  } from 'lucide-svelte';
  import { rpc } from '../lib/rpc.svelte';
  import type { KanbanTicket } from '../lib/types';

  let tickets = $state<KanbanTicket[]>([]);
  let selectedTicketId = $state<string>('');
  let loadingTickets = $state(false);
  let savedStatus = $state(false);

  let scriptText = $state(
`[0:00 - 0:15 HOOK]
When you run a command in Linux, you aren't just starting a process. You are requesting the kernel to slice CPU time, carve virtual memory tables, and isolate your binary in an unbreachable hardware sandbox.

[0:15 - 0:35 EXPLICIT PAYOFF]
In the next 10 minutes, we are going to trace the exact journey of a Linux syscall from user space registers to ring 0 kernel memory. You will understand how page tables, context switches, and isolation actually work under the hood.

[0:35 - 1:00 MENTAL MODEL]
Imagine user space as an isolated container that has zero direct access to physical RAM. Every read or write must pass through the MMU and kernel gate. Let's inspect what happens during the very first instruction.`
  );

  let isPlaying = $state(false);
  let wpm = $state(140);
  let fontSize = $state(28);
  let isFullscreen = $state(false);
  let elapsedSeconds = $state(0);
  let scrollContainer: HTMLElement | null = $state(null);

  let animationFrameId: number | null = null;
  let timerInterval: any = null;

  async function loadKanbanTickets() {
    loadingTickets = true;
    try {
      const res = await rpc.call('kanban.list', {});
      if (res && res.tickets) {
        tickets = res.tickets;
      }
    } catch (e) {
      console.error('Failed to load tickets in teleprompter:', e);
    } finally {
      loadingTickets = false;
    }
  }

  async function handleSelectTicket(id: string) {
    selectedTicketId = id;
    if (!id) return;
    try {
      const res = await rpc.call('kanban.prompt', { ticket_id: id });
      if (res && res.prompt) {
        scriptText = res.prompt;
        resetAll();
      }
    } catch (e) {
      console.error('Failed to fetch ticket prompt:', e);
    }
  }

  function togglePlay() {
    isPlaying = !isPlaying;
    if (isPlaying) {
      startScrolling();
      startTimer();
    } else {
      stopScrolling();
      stopTimer();
    }
  }

  function startTimer() {
    if (timerInterval) return;
    timerInterval = setInterval(() => {
      elapsedSeconds += 1;
    }, 1000);
  }

  function stopTimer() {
    if (timerInterval) {
      clearInterval(timerInterval);
      timerInterval = null;
    }
  }

  function resetAll() {
    isPlaying = false;
    stopScrolling();
    stopTimer();
    elapsedSeconds = 0;
    if (scrollContainer) {
      scrollContainer.scrollTop = 0;
    }
  }

  function startScrolling() {
    let lastTime = performance.now();

    function step(currentTime: number) {
      if (!isPlaying || !scrollContainer) return;

      const delta = (currentTime - lastTime) / 1000;
      lastTime = currentTime;

      // Calculate scroll velocity from WPM:
      // Approx 5 chars per word, 40-60px per line of text
      const pixelsPerSecond = (wpm / 60) * (fontSize * 1.3);
      scrollContainer.scrollTop += pixelsPerSecond * delta;

      animationFrameId = requestAnimationFrame(step);
    }

    animationFrameId = requestAnimationFrame(step);
  }

  function stopScrolling() {
    if (animationFrameId !== null) {
      cancelAnimationFrame(animationFrameId);
      animationFrameId = null;
    }
  }

  function toggleFullscreen() {
    isFullscreen = !isFullscreen;
    if (isFullscreen) {
      document.documentElement.requestFullscreen?.().catch(() => {});
    } else {
      document.exitFullscreen?.().catch(() => {});
    }
  }

  function handleKeyDown(e: KeyboardEvent) {
    if (e.code === 'Space' && (e.target as HTMLElement).tagName !== 'TEXTAREA') {
      e.preventDefault();
      togglePlay();
    }
  }

  onMount(() => {
    window.addEventListener('keydown', handleKeyDown);
  });

  onDestroy(() => {
    window.removeEventListener('keydown', handleKeyDown);
    stopScrolling();
    stopTimer();
  });

  const formattedTime = $derived(
    `${Math.floor(elapsedSeconds / 60).toString().padStart(2, '0')}:${(elapsedSeconds % 60).toString().padStart(2, '0')}`
  );

  const estimatedTotalTime = $derived(() => {
    const wordCount = scriptText.trim().split(/\s+/).length;
    const minutes = wordCount / wpm;
    return `${Math.floor(minutes)}m ${Math.round((minutes % 1) * 60)}s`;
  });
</script>

<div class="space-y-6 {isFullscreen ? 'fixed inset-0 z-50 bg-black p-8 overflow-hidden' : ''}">

  <!-- Control HUD -->
  <div class="p-4 rounded-2xl bg-gray-900/80 border border-gray-800 flex flex-wrap items-center justify-between gap-4 backdrop-blur-md">
    
    <!-- Playback Controls -->
    <div class="flex items-center space-x-2">
      <button
        onclick={togglePlay}
        class="inline-flex items-center space-x-2 px-4 py-2 rounded-xl {isPlaying ? 'bg-amber-600 hover:bg-amber-500' : 'bg-emerald-600 hover:bg-emerald-500'} text-white text-xs font-bold transition-all shadow-lg cursor-pointer"
      >
        {#if isPlaying}
          <Pause class="w-4 h-4" />
          <span>Pause (Space)</span>
        {:else}
          <Play class="w-4 h-4" />
          <span>Start Teleprompter (Space)</span>
        {/if}
      </button>

      <button
        onclick={resetAll}
        class="p-2 rounded-xl bg-gray-800 hover:bg-gray-700 text-gray-300 transition-colors cursor-pointer"
        title="Reset Scroll & Timer"
      >
        <RotateCcw class="w-4 h-4" />
      </button>
    </div>

    <!-- Speed & Typography Controls -->
    <div class="flex items-center space-x-4 text-xs text-gray-300">
      
      <!-- WPM Control -->
      <div class="flex items-center space-x-2">
        <Gauge class="w-4 h-4 text-indigo-400" />
        <span class="font-mono">{wpm} WPM</span>
        <input 
          type="range" 
          min="80" 
          max="260" 
          step="5"
          bind:value={wpm} 
          class="w-24 accent-indigo-500 cursor-pointer"
        />
      </div>

      <!-- Font Size Control -->
      <div class="flex items-center space-x-2">
        <Type class="w-4 h-4 text-purple-400" />
        <span class="font-mono">{fontSize}px</span>
        <input 
          type="range" 
          min="18" 
          max="48" 
          step="2"
          bind:value={fontSize} 
          class="w-20 accent-purple-500 cursor-pointer"
        />
      </div>

      <!-- Timer HUD -->
      <div class="flex items-center space-x-1.5 px-3 py-1 rounded-lg bg-gray-950 border border-gray-800 font-mono text-xs">
        <Clock class="w-3.5 h-3.5 text-emerald-400" />
        <span class="text-emerald-400 font-bold">{formattedTime}</span>
        <span class="text-gray-600">/</span>
        <span class="text-gray-400">{estimatedTotalTime()} est</span>
      </div>

      <!-- Fullscreen Toggle -->
      <button
        onclick={toggleFullscreen}
        class="p-2 rounded-xl bg-gray-800 hover:bg-gray-700 text-gray-300 transition-colors cursor-pointer"
        title="Toggle Fullscreen Focus"
      >
        {#if isFullscreen}
          <Minimize2 class="w-4 h-4" />
        {:else}
          <Maximize2 class="w-4 h-4" />
        {/if}
      </button>
    </div>

  </div>

  <!-- Studio Main Canvas -->
  <div class="grid grid-cols-1 lg:grid-cols-12 gap-6 {isFullscreen ? 'h-[calc(100vh-100px)]' : 'h-[600px]'}">
    
    <!-- Script Editor Side (Hidden in Fullscreen) -->
    {#if !isFullscreen}
      <div class="lg:col-span-5 flex flex-col space-y-2">
        <div class="flex items-center justify-between text-xs text-gray-400 font-medium">
          <span class="flex items-center space-x-1.5">
            <FileText class="w-4 h-4 text-indigo-400" />
            <span>Script Blueprint Editor</span>
          </span>
          <span>{scriptText.trim().split(/\s+/).length} words</span>
        </div>
        <textarea
          bind:value={scriptText}
          placeholder="Paste or write your script with [HOOK], [PAYOFF], [RETENTION] markers..."
          class="w-full flex-1 p-4 rounded-2xl bg-gray-950 border border-gray-800 text-gray-200 font-mono text-xs focus:outline-none focus:border-indigo-500 leading-relaxed resize-none"
        ></textarea>
      </div>
    {/if}

    <!-- Live Teleprompter Reading Rail -->
    <div class="{isFullscreen ? 'col-span-12' : 'lg:col-span-7'} relative rounded-2xl bg-black border border-gray-900 overflow-hidden flex flex-col">
      
      <!-- Eye-Level Reading Cue Line -->
      <div class="absolute top-1/3 left-0 right-0 h-14 bg-indigo-500/10 border-y border-indigo-500/20 pointer-events-none z-20 flex items-center justify-between px-4">
        <span class="text-[10px] font-mono uppercase tracking-widest text-indigo-400 font-bold">
          ◀ Camera Eye Line
        </span>
        <span class="text-[10px] font-mono text-indigo-400 font-bold">
          60 FPS Native Easing
        </span>
      </div>

      <!-- Scrolling Viewport -->
      <div 
        bind:this={scrollContainer}
        class="flex-1 overflow-y-auto px-8 py-48 text-center scroll-smooth select-none"
        style="font-size: {fontSize}px; line-height: 1.8;"
      >
        <div class="max-w-3xl mx-auto font-sans font-semibold text-gray-100 space-y-8 tracking-wide">
          {#each scriptText.split('\n\n') as block}
            {#if block.startsWith('[')}
              <div class="text-xs font-mono font-bold text-indigo-400 tracking-widest uppercase bg-indigo-950/40 py-1.5 px-3 rounded-md inline-block border border-indigo-500/30">
                {block.split(']')[0]}]
              </div>
              <p class="text-gray-200">
                {block.split(']').slice(1).join(']').trim()}
              </p>
            {:else}
              <p class="text-gray-200">
                {block}
              </p>
            {/if}
          {/each}
        </div>
      </div>

    </div>

  </div>

</div>
