<script lang="ts">
  import type { KeywordResearch } from '../lib/types';
  import { FileText, Tag, Hash, ShieldCheck, ChevronDown, ChevronUp, Database } from 'lucide-svelte';

  let { research }: { research: KeywordResearch } = $props();

  let isOpen = $state(true);
</script>

<div class="rounded-xl bg-gray-900/50 border border-gray-800 overflow-hidden">
  <!-- Header Toggle -->
  <button 
    onclick={() => isOpen = !isOpen}
    class="w-full px-4 py-3 bg-gray-900/80 border-b border-gray-800 flex items-center justify-between hover:bg-gray-800/50 transition-colors"
  >
    <div class="flex items-center space-x-2.5">
      <div class="w-6 h-6 rounded-md bg-indigo-500/10 border border-indigo-500/20 flex items-center justify-center">
        <ShieldCheck class="w-3.5 h-3.5 text-indigo-400" />
      </div>
      <div class="text-left">
        <span class="text-xs font-semibold text-gray-200 uppercase tracking-wider">
          Verifiable Evidence Ledger
        </span>
        <span class="ml-2 text-[11px] text-gray-400">
          Pure SQLite BM25 + Live SERP Attribution
        </span>
      </div>
    </div>
    
    <div class="text-gray-400">
      {#if isOpen}
        <ChevronUp class="w-4 h-4" />
      {:else}
        <ChevronDown class="w-4 h-4" />
      {/if}
    </div>
  </button>

  {#if isOpen}
    <div class="p-4 space-y-4 text-xs">
      
      <!-- Metrics Summary Grid -->
      <div class="grid grid-cols-2 sm:grid-cols-4 gap-3">
        <div class="p-3 rounded-lg bg-gray-950/60 border border-gray-800">
          <div class="text-[11px] text-gray-400">SERP Competition</div>
          <div class="mt-1 text-base font-bold text-gray-100 font-mono">
            {research.competition_score.toFixed(1)} / 100
          </div>
        </div>
        <div class="p-3 rounded-lg bg-gray-950/60 border border-gray-800">
          <div class="text-[11px] text-gray-400">Opportunity Score</div>
          <div class="mt-1 text-base font-bold text-emerald-400 font-mono">
            {research.opportunity_score.toFixed(1)} / 100
          </div>
        </div>
        <div class="p-3 rounded-lg bg-gray-950/60 border border-gray-800">
          <div class="text-[11px] text-gray-400">SERP Mean Views</div>
          <div class="mt-1 text-base font-bold text-gray-100 font-mono">
            {research.serp_mean_views.toLocaleString()}
          </div>
        </div>
        <div class="p-3 rounded-lg bg-gray-950/60 border border-gray-800">
          <div class="text-[11px] text-gray-400">Ranking Channels</div>
          <div class="mt-1 text-base font-bold text-indigo-400 font-mono">
            {research.ranking_channels} unique
          </div>
        </div>
      </div>

      <!-- Suggested Tag Frequencies -->
      {#if research.suggested_tags && research.suggested_tags.length > 0}
        <div>
          <div class="flex items-center space-x-1.5 text-gray-300 font-medium mb-2">
            <Tag class="w-3.5 h-3.5 text-indigo-400" />
            <span>Harvested Competitor Tag Signals ({research.suggested_tags.length})</span>
          </div>
          <div class="flex flex-wrap gap-1.5">
            {#each research.suggested_tags as tag}
              <span class="inline-flex items-center space-x-1 px-2.5 py-1 rounded-md bg-gray-950 border border-gray-800 text-gray-300 text-[11px]">
                <span>{tag.tag}</span>
                <span class="text-indigo-400 font-mono font-semibold ml-1">×{tag.usage}</span>
              </span>
            {/each}
          </div>
        </div>
      {/if}

      <!-- Corpus BM25 Resonance -->
      {#if research.corpus_matches && research.corpus_matches.length > 0}
        <div>
          <div class="flex items-center space-x-1.5 text-gray-300 font-medium mb-2">
            <Database class="w-3.5 h-3.5 text-indigo-400" />
            <span>Corpus BM25 Semantic Resonance Matches</span>
          </div>
          <div class="space-y-1.5 max-h-40 overflow-y-auto pr-1">
            {#each research.corpus_matches as doc}
              <div class="p-2 rounded-md bg-gray-950/80 border border-gray-800/80 flex items-center justify-between">
                <span class="text-gray-300 truncate max-w-[80%] font-medium">{doc.title}</span>
                <span class="font-mono text-indigo-400 text-[11px] bg-indigo-500/10 px-2 py-0.5 rounded border border-indigo-500/20">
                  BM25 {doc.bm25.toFixed(2)}
                </span>
              </div>
            {/each}
          </div>
        </div>
      {/if}

    </div>
  {/if}
</div>
