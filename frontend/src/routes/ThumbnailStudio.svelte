<script lang="ts">
  import { onMount } from 'svelte';
  import { Image, Sparkles, Download, Palette, Type, Layout, Kanban, Check } from 'lucide-svelte';
  import { rpc } from '../lib/rpc.svelte';
  import type { KanbanTicket } from '../lib/types';

  let tickets = $state<KanbanTicket[]>([]);
  let selectedTicketId = $state<string>('');

  let titleLine1 = $state('HOW LINUX RUNS CODE');
  let titleLine2 = $state('Inside Syscalls & Memory');
  let badgeText = $state('KERNEL ARCHITECTURE');
  let theme = $state<'neon' | 'dark' | 'cyber' | 'minimal'>('neon');
  let subtitleText = $state('0 to Ring-0 Explained');
  let downloading = $state(false);

  const themes = [
    { id: 'neon', label: 'Neon Cyber', bg: 'from-black via-indigo-950 to-black', accent: 'text-indigo-400', hexAccent: '#818cf8', border: 'border-indigo-500/30' },
    { id: 'dark', label: 'Pure Dark Minimal', bg: 'from-gray-950 to-black', accent: 'text-emerald-400', hexAccent: '#34d399', border: 'border-emerald-500/30' },
    { id: 'cyber', label: 'High-Voltage Amber', bg: 'from-black via-amber-950/40 to-black', accent: 'text-amber-400', hexAccent: '#fbbf24', border: 'border-amber-500/30' },
    { id: 'minimal', label: 'Monochrome Slate', bg: 'from-slate-950 to-black', accent: 'text-rose-400', hexAccent: '#fb7185', border: 'border-rose-500/30' },
  ];

  const activeThemeObj = $derived(themes.find(t => t.id === theme) || themes[0]);

  async function loadTickets() {
    try {
      const res = await rpc.call('kanban.list', {});
      if (res && res.tickets) {
        tickets = res.tickets;
      }
    } catch (e) {
      console.error('Failed to load tickets in thumbnail studio:', e);
    }
  }

  function handleSelectTicket(id: string) {
    selectedTicketId = id;
    const t = tickets.find(x => x.ticket_id === id);
    if (!t) return;

    badgeText = (t.channel || 'TECHVERSE').toUpperCase();
    const clean = t.title.replace(/:/g, ' — ');
    const parts = clean.split(/[—\(\)]/).map(s => s.trim()).filter(Boolean);
    if (parts.length >= 2) {
      titleLine1 = parts[0].toUpperCase();
      titleLine2 = parts[1];
      subtitleText = parts[2] || t.framework || 'Deep Dive Engineering';
    } else {
      titleLine1 = clean.toUpperCase();
      titleLine2 = t.framework || 'Architecture Breakdown';
      subtitleText = t.target_keyword || 'Core Mental Model';
    }
  }

  function downloadThumbnail() {
    downloading = true;
    try {
      const canvas = document.createElement('canvas');
      canvas.width = 1280;
      canvas.height = 720;
      const ctx = canvas.getContext('2d');
      if (!ctx) return;

      // 1. Background Gradient
      const grad = ctx.createLinearGradient(0, 0, 1280, 720);
      if (theme === 'neon') {
        grad.addColorStop(0, '#030712');
        grad.addColorStop(0.5, '#1e1b4b');
        grad.addColorStop(1, '#000000');
      } else if (theme === 'dark') {
        grad.addColorStop(0, '#030712');
        grad.addColorStop(0.5, '#064e3b');
        grad.addColorStop(1, '#000000');
      } else if (theme === 'cyber') {
        grad.addColorStop(0, '#030712');
        grad.addColorStop(0.5, '#451a03');
        grad.addColorStop(1, '#000000');
      } else {
        grad.addColorStop(0, '#020617');
        grad.addColorStop(0.5, '#4c0519');
        grad.addColorStop(1, '#000000');
      }
      ctx.fillStyle = grad;
      ctx.fillRect(0, 0, 1280, 720);

      // Subtle cyber grid
      ctx.strokeStyle = 'rgba(255, 255, 255, 0.04)';
      ctx.lineWidth = 1;
      for (let x = 0; x < 1280; x += 80) {
        ctx.beginPath();
        ctx.moveTo(x, 0);
        ctx.lineTo(x, 720);
        ctx.stroke();
      }
      for (let y = 0; y < 720; y += 80) {
        ctx.beginPath();
        ctx.moveTo(0, y);
        ctx.lineTo(1280, y);
        ctx.stroke();
      }

      // 2. Badge
      ctx.fillStyle = 'rgba(255, 255, 255, 0.12)';
      ctx.strokeStyle = 'rgba(255, 255, 255, 0.25)';
      ctx.lineWidth = 2;
      const badgeWidth = ctx.measureText(badgeText).width + 60;
      ctx.beginPath();
      ctx.roundRect(80, 70, Math.max(badgeWidth, 240), 48, 24);
      ctx.fill();
      ctx.stroke();

      ctx.fillStyle = '#ffffff';
      ctx.font = 'bold 18px monospace';
      ctx.fillText(badgeText, 105, 101);

      // 3. Title Line 1 (White Bold)
      ctx.fillStyle = '#ffffff';
      ctx.font = '900 64px system-ui, -apple-system, sans-serif';
      ctx.fillText(titleLine1, 80, 310);

      // 4. Title Line 2 (Accent Color)
      ctx.fillStyle = activeThemeObj.hexAccent;
      ctx.font = '900 56px system-ui, -apple-system, sans-serif';
      ctx.fillText(titleLine2, 80, 395);

      // 5. Subtitle & Branding
      ctx.fillStyle = '#9ca3af';
      ctx.font = '500 24px monospace';
      ctx.fillText(subtitleText, 80, 640);

      ctx.fillStyle = '#4b5563';
      ctx.font = 'bold 18px monospace';
      ctx.fillText('TUBEFORGE 1280x720 CTR ENGINE', 840, 640);

      // Trigger download
      const dataUrl = canvas.toDataURL('image/png');
      const a = document.createElement('a');
      a.href = dataUrl;
      a.download = `thumbnail-${titleLine1.toLowerCase().replace(/[^a-z0-9]+/g, '-')}.png`;
      a.click();
    } catch (e) {
      console.error('Failed to export thumbnail PNG:', e);
    } finally {
      setTimeout(() => { downloading = false; }, 800);
    }
  }

  onMount(() => {
    loadTickets();
  });
</script>

<div class="space-y-6">

  <!-- Header -->
  <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4 bg-gray-900/60 border border-gray-800 p-5 rounded-2xl">
    <div>
      <div class="flex items-center space-x-2">
        <Image class="w-5 h-5 text-indigo-400" />
        <h2 class="text-xl font-extrabold text-white tracking-tight">
          1280x720 Thumbnail Canvas Studio
        </h2>
      </div>
      <p class="text-xs text-gray-400 mt-1">
        Design high-CTR, mobile-optimized YouTube thumbnails with high-contrast typography, live Kanban ticket loading, and 1280x720 PNG export.
      </p>
    </div>

    <!-- Ticket Selector Dropdown & Download Button -->
    <div class="flex items-center space-x-3">
      <select
        value={selectedTicketId}
        onchange={(e) => handleSelectTicket((e.target as HTMLSelectElement).value)}
        class="px-3 py-2 rounded-xl bg-gray-950 border border-gray-800 text-gray-200 text-xs focus:outline-none focus:border-indigo-500 cursor-pointer max-w-xs"
      >
        <option value="">-- Load from Kanban Ticket --</option>
        {#each tickets as ticket}
          <option value={ticket.ticket_id}>
            [{ticket.channel}] {ticket.title}
          </option>
        {/each}
      </select>

      <button
        type="button"
        onclick={downloadThumbnail}
        disabled={downloading}
        class="inline-flex items-center space-x-2 px-4 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-bold shadow-lg shadow-indigo-500/25 transition-all cursor-pointer"
      >
        <Download class="w-4 h-4" />
        <span>{downloading ? 'Rendering PNG...' : 'Export 1280x720 PNG'}</span>
      </button>
    </div>
  </div>

  <div class="grid grid-cols-1 lg:grid-cols-12 gap-6">
    
    <!-- Controls Side -->
    <div class="lg:col-span-4 p-5 rounded-2xl bg-gray-900/60 border border-gray-800 space-y-4">
      <div class="text-xs font-bold text-gray-300 uppercase tracking-wider flex items-center space-x-2">
        <Palette class="w-4 h-4 text-indigo-400" />
        <span>Design Controls</span>
      </div>

      <div class="space-y-3 text-xs">
        <div>
          <label for="thumb-badge" class="block text-gray-400 mb-1 font-medium">Header Badge</label>
          <input 
            id="thumb-badge"
            type="text" 
            bind:value={badgeText}
            class="w-full px-3 py-2 rounded-lg bg-gray-950 border border-gray-800 text-gray-100 font-mono text-xs focus:outline-none focus:border-indigo-500"
          />
        </div>

        <div>
          <label for="thumb-line1" class="block text-gray-400 mb-1 font-medium">Main Title (Line 1)</label>
          <input 
            id="thumb-line1"
            type="text" 
            bind:value={titleLine1}
            class="w-full px-3 py-2 rounded-lg bg-gray-950 border border-gray-800 text-gray-100 font-bold text-xs focus:outline-none focus:border-indigo-500"
          />
        </div>

        <div>
          <label for="thumb-line2" class="block text-gray-400 mb-1 font-medium">Main Title (Line 2)</label>
          <input 
            id="thumb-line2"
            type="text" 
            bind:value={titleLine2}
            class="w-full px-3 py-2 rounded-lg bg-gray-950 border border-gray-800 text-gray-100 font-bold text-xs focus:outline-none focus:border-indigo-500"
          />
        </div>

        <div>
          <label for="thumb-sub" class="block text-gray-400 mb-1 font-medium">Sub-Payoff Hook</label>
          <input 
            id="thumb-sub"
            type="text" 
            bind:value={subtitleText}
            class="w-full px-3 py-2 rounded-lg bg-gray-950 border border-gray-800 text-gray-100 text-xs focus:outline-none focus:border-indigo-500"
          />
        </div>

        <div>
          <span class="block text-gray-400 mb-1 font-medium">Theme Style</span>
          <div class="grid grid-cols-2 gap-2">
            {#each themes as t}
              <button
                type="button"
                onclick={() => theme = t.id as any}
                class="px-3 py-2 rounded-lg text-xs font-semibold border transition-all cursor-pointer {
                  theme === t.id 
                    ? 'bg-indigo-600/20 text-indigo-300 border-indigo-500/60' 
                    : 'bg-gray-950 text-gray-400 border-gray-800 hover:border-gray-700'
                }"
              >
                {t.label}
              </button>
            {/each}
          </div>
        </div>
      </div>
    </div>

    <!-- Live Preview Canvas -->
    <div class="lg:col-span-8 flex flex-col space-y-3">
      <div class="flex items-center justify-between text-xs text-gray-400 font-medium">
        <span>16:9 High-DPI Preview (1280 × 720)</span>
        <span class="font-mono text-emerald-400">Mobile Legibility — 100%</span>
      </div>

      <!-- Canvas Frame -->
      <div class="relative w-full aspect-video rounded-2xl bg-gradient-to-br {activeThemeObj.bg} border {activeThemeObj.border} overflow-hidden shadow-2xl p-8 sm:p-12 flex flex-col justify-between select-none">
        
        <!-- Header Tag -->
        <div class="flex items-center space-x-3">
          <span class="px-3.5 py-1 rounded-full bg-white/10 backdrop-blur-md border border-white/20 text-white font-mono font-extrabold text-xs sm:text-sm tracking-widest uppercase">
            {badgeText}
          </span>
        </div>

        <!-- Center Kinetic Typography -->
        <div class="space-y-1 sm:space-y-2">
          <h1 class="text-3xl sm:text-5xl font-black text-white tracking-tight uppercase drop-shadow-md">
            {titleLine1}
          </h1>
          <h2 class="text-2xl sm:text-4xl font-black {activeThemeObj.accent} tracking-tight drop-shadow-md">
            {titleLine2}
          </h2>
        </div>

        <!-- Footer Hook -->
        <div class="flex items-center justify-between">
          <span class="text-xs sm:text-sm font-mono text-gray-400 tracking-wider">
            {subtitleText}
          </span>
          <span class="text-[10px] font-mono text-gray-600 uppercase">
            TubeForge Visual Engine
          </span>
        </div>

      </div>

    </div>

  </div>

</div>
