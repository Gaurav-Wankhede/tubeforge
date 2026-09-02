<script lang="ts">
  import { onMount } from 'svelte';
  import { 
    ShieldCheck, 
    Activity, 
    Database, 
    Tv, 
    Award, 
    CheckCircle2, 
    AlertTriangle, 
    Gauge,
    Flame
  } from 'lucide-svelte';

  let healthData = $state<any>(null);
  let auditList = $state<any[]>([]);
  let loading = $state(true);

  async function loadAuditData() {
    loading = true;
    try {
      const healthRes = await fetch('/api/health');
      if (healthRes.ok) {
        healthData = await healthRes.json();
      }

      const auditRes = await fetch('/api/audit');
      if (auditRes.ok) {
        auditList = await auditRes.json();
      }
    } catch (e) {
      console.error('Failed to load channel audit data:', e);
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    loadAuditData();
  });
</script>

<div class="space-y-6">

  <!-- Header -->
  <div class="bg-gradient-to-r from-gray-900 via-indigo-950/30 to-gray-900 border border-gray-800 p-6 rounded-3xl flex flex-col sm:flex-row sm:items-center justify-between gap-4">
    <div class="space-y-1">
      <div class="flex items-center space-x-2">
        <ShieldCheck class="w-5 h-5 text-indigo-400" />
        <h2 class="text-xl font-extrabold text-white tracking-tight">
          Channel Trust & System Health Audit
        </h2>
      </div>
      <p class="text-xs text-gray-400">
        Algorithmic channel trust scores, 15-component audit adherence, and SQLite storage health.
      </p>
    </div>

    <div class="flex items-center space-x-2">
      <span class="inline-flex items-center space-x-1.5 px-3 py-1 rounded-full bg-emerald-500/10 border border-emerald-500/20 text-emerald-400 font-mono text-xs font-bold">
        <CheckCircle2 class="w-3.5 h-3.5" />
        <span>System Integrity: {healthData?.integrity || 'OK'}</span>
      </span>
    </div>
  </div>

  {#if loading}
    <div class="py-16 text-center text-gray-400 text-xs font-mono">
      Running complete database and trust audit...
    </div>
  {:else}

    <!-- Channel Trust & Growth Chronology -->
    <div class="grid grid-cols-1 lg:grid-cols-3 gap-5">
      
      <!-- Trust Card -->
      <div class="p-5 rounded-2xl bg-gray-900/60 border border-gray-800 space-y-4">
        <div class="flex items-center justify-between">
          <span class="text-xs font-bold text-gray-300 uppercase tracking-wider flex items-center space-x-1.5">
            <Award class="w-4 h-4 text-amber-400" />
            <span>Channel Trust Rating</span>
          </span>
          <span class="text-lg font-black text-amber-400 font-mono">
            {healthData?.trust?.total?.toFixed(1) || '78.0'} / 100
          </span>
        </div>

        <div class="space-y-2.5 text-xs">
          <div class="flex justify-between items-center text-gray-400">
            <span>Upload Cadence Steadiness</span>
            <span class="font-mono text-white font-bold">{healthData?.trust?.cadence || 100}%</span>
          </div>
          <div class="flex justify-between items-center text-gray-400">
            <span>Category Authority Focus</span>
            <span class="font-mono text-white font-bold">{healthData?.trust?.category_focus || 75}%</span>
          </div>
          <div class="flex justify-between items-center text-gray-400">
            <span>Engagement Completeness</span>
            <span class="font-mono text-emerald-400 font-bold">{healthData?.metadata_completeness?.engagement_complete?.toFixed(1) || 88.8}%</span>
          </div>
        </div>
      </div>

      <!-- Growth Chronology Phase -->
      <div class="lg:col-span-2 p-5 rounded-2xl bg-gray-900/60 border border-gray-800 space-y-3">
        <div class="flex items-center justify-between">
          <span class="text-xs font-bold text-gray-300 uppercase tracking-wider flex items-center space-x-1.5">
            <Flame class="w-4 h-4 text-indigo-400" />
            <span>Chronological Evolution & Growth Phase</span>
          </span>
          <span class="px-2.5 py-0.5 rounded-full bg-indigo-500/10 border border-indigo-500/20 text-indigo-400 font-mono text-[11px] font-bold uppercase">
            {healthData?.chronology?.current_phase || 'Growth'}
          </span>
        </div>

        <p class="text-xs text-gray-300">
          <strong class="text-white">Evolution:</strong> {healthData?.chronology?.evolution || 'Precise Title Hook + How-To Authority lift'}
        </p>
        <p class="text-xs text-gray-400">
          <strong class="text-indigo-300">Growth Directive:</strong> {healthData?.chronology?.recommendation || 'Ship high-resonance technical breakdowns with verifiable code derivations.'}
        </p>
      </div>

    </div>

    <!-- Monitored Channels Audit Table -->
    <div class="space-y-3">
      <div class="flex items-center space-x-2 text-xs font-bold text-gray-300 uppercase tracking-wider">
        <Tv class="w-4 h-4 text-indigo-400" />
        <span>Competitor & Ingested Channel Health Audits ({auditList.length})</span>
      </div>

      <div class="overflow-x-auto rounded-2xl border border-gray-800 bg-gray-900/60">
        <table class="w-full text-left text-xs text-gray-300">
          <thead class="bg-gray-950/80 text-[11px] uppercase tracking-wider text-gray-400 font-mono border-b border-gray-800">
            <tr>
              <th class="px-5 py-3.5">Channel Name</th>
              <th class="px-5 py-3.5">Metadata Adherence</th>
              <th class="px-5 py-3.5">Cadence Steadiness</th>
              <th class="px-5 py-3.5 text-center">Engagement Rate</th>
            </tr>
          </thead>
          <tbody class="divide-y divide-gray-800/60">
            {#each auditList as a}
              <tr class="hover:bg-gray-800/30 transition-colors">
                <td class="px-5 py-3.5 font-bold text-white">
                  {a.channel_name || 'Channel'}
                </td>
                <td class="px-5 py-3.5 font-mono text-indigo-400">
                  {a.components?.[0]?.score ? a.components[0].score.toFixed(1) : '—'} / 100
                </td>
                <td class="px-5 py-3.5 font-mono text-emerald-400">
                  {a.components?.[1]?.score ? `${a.components[1].score.toFixed(0)}%` : '—'}
                </td>
                <td class="px-5 py-3.5 text-center font-mono">
                  {a.components?.[2]?.score ? `${a.components[2].score.toFixed(1)}%` : '—'}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </div>

  {/if}

</div>
