<script lang="ts">
  import { onMount } from 'svelte';
  import { rpc } from '../lib/rpc.svelte';
  import type { KanbanTicket } from '../lib/types';
  import { 
    Kanban, 
    Plus, 
    Trash2, 
    FileText, 
    Clock, 
    Copy,
    Check,
    X
  } from 'lucide-svelte';

  let tickets = $state<KanbanTicket[]>([]);
  let loading = $state(true);
  let showNewModal = $state(false);
  let activePromptTicket = $state<KanbanTicket | null>(null);
  let activePromptText = $state<string | null>(null);
  let promptCopied = $state(false);

  // New ticket form
  let newTitle = $state('');
  let newTopic = $state('');
  let newChannel = $state('TECHVERSE');
  let newFramework = $state('Core Mental Model');
  let newDuration = $state(720);

  const columns: Array<{ id: 'todo' | 'inprogress' | 'done' | 'published'; label: string; color: string }> = [
    { id: 'todo', label: 'To-Do', color: 'border-indigo-500/40' },
    { id: 'inprogress', label: 'Scripting & Recording', color: 'border-amber-500/40' },
    { id: 'done', label: 'Ready for Publish', color: 'border-emerald-500/40' },
    { id: 'published', label: 'Live on YouTube', color: 'border-purple-500/40' },
  ];

  async function loadTickets() {
    loading = true;
    try {
      const res = await rpc.call('kanban.list', {});
      if (res && res.tickets) {
        tickets = res.tickets;
      }
    } catch {
      // ignore
    } finally {
      loading = false;
    }
  }

  async function moveTicket(ticketId: string, nextStatus: 'todo' | 'inprogress' | 'done' | 'published') {
    try {
      await rpc.call('kanban.move', {
        ticket_id: ticketId,
        status: nextStatus,
      });
      await loadTickets();
    } catch {
      // ignore
    }
  }

  async function deleteTicket(ticketId: string) {
    try {
      await rpc.call('kanban.delete', { ticket_id: ticketId });
      await loadTickets();
    } catch {
      // ignore
    }
  }

  async function createTicket() {
    if (!newTitle.trim()) return;
    try {
      await rpc.call('kanban.create', {
        title: newTitle,
        topic: newTopic || newTitle,
        channel: newChannel,
        framework: newFramework,
        optimal_duration_sec: Number(newDuration),
        status: 'todo',
      });
      showNewModal = false;
      newTitle = '';
      newTopic = '';
      await loadTickets();
    } catch {
      // ignore
    }
  }

  async function openPromptModal(ticket: KanbanTicket) {
    activePromptTicket = ticket;
    activePromptText = 'Generating First-Screen retention prompt contract...';
    promptCopied = false;
    try {
      const res = await rpc.call('kanban.prompt', { ticket_id: ticket.ticket_id });
      if (res && res.prompt) {
        activePromptText = res.prompt;
      }
    } catch (e: any) {
      activePromptText = `Failed to generate prompt — ${e.message}`;
    }
  }

  function copyPrompt() {
    if (!activePromptText) return;
    navigator.clipboard.writeText(activePromptText);
    promptCopied = true;
    setTimeout(() => promptCopied = false, 2000);
  }

  onMount(() => {
    loadTickets();
  });
</script>

<div class="space-y-6">

  <!-- Header -->
  <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
    <div>
      <h2 class="text-xl font-extrabold text-white tracking-tight">
        Production Kanban Board
      </h2>
      <p class="text-xs text-gray-400">
        Track video pipeline execution from topic research to 0:00–0:45 First-Screen blueprints and publishing.
      </p>
    </div>

    <button
      type="button"
      onclick={() => showNewModal = true}
      class="inline-flex items-center space-x-2 px-4 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-bold shadow-lg shadow-indigo-500/20 transition-all cursor-pointer"
    >
      <Plus class="w-4 h-4" />
      <span>New Production Ticket</span>
    </button>
  </div>

  <!-- Board Columns Grid -->
  <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
    {#each columns as col}
      {@const colTickets = tickets.filter(t => t.status === col.id)}
      
      <div class="rounded-xl bg-gray-900/40 border {col.color} flex flex-col min-h-[500px]">
        <!-- Column Header -->
        <div class="p-3.5 border-b border-gray-800/80 flex items-center justify-between">
          <div class="flex items-center space-x-2">
            <span class="text-xs font-bold text-gray-200">{col.label}</span>
            <span class="px-2 py-0.5 rounded-full text-[10px] font-mono font-bold bg-gray-800 text-gray-400">
              {colTickets.length}
            </span>
          </div>
        </div>

        <!-- Ticket Cards Container -->
        <div class="p-3 flex-1 space-y-3 overflow-y-auto max-h-[700px]">
          {#if colTickets.length === 0}
            <div class="p-4 text-center text-xs text-gray-600 border border-dashed border-gray-800/60 rounded-lg">
              No tickets in {col.label.toLowerCase()}
            </div>
          {/if}

          {#each colTickets as ticket}
            <div class="p-3.5 rounded-xl bg-gray-950/80 border border-gray-800 hover:border-indigo-500/40 transition-all shadow-sm flex flex-col justify-between space-y-3">
              <div>
                <div class="flex items-center justify-between text-[10px] text-gray-400 font-mono mb-1.5">
                  <span class="px-1.5 py-0.5 rounded bg-indigo-500/10 text-indigo-400 border border-indigo-500/20">
                    {ticket.channel}
                  </span>
                  {#if ticket.optimal_duration_sec}
                    <span class="flex items-center space-x-1 text-gray-500">
                      <Clock class="w-3 h-3" />
                      <span>{Math.round(ticket.optimal_duration_sec / 60)}m</span>
                    </span>
                  {/if}
                </div>

                <h4 class="text-xs font-semibold text-gray-100 line-clamp-2 leading-relaxed">
                  {ticket.title}
                </h4>

                {#if ticket.framework}
                  <div class="mt-2 text-[10px] text-gray-400">
                    <span class="text-gray-500">Framework —</span> {ticket.framework}
                  </div>
                {/if}
              </div>

              <!-- Action Bar -->
              <div class="pt-2 border-t border-gray-900 flex items-center justify-between text-xs">
                <button
                  type="button"
                  onclick={() => openPromptModal(ticket)}
                  class="inline-flex items-center space-x-1 text-[11px] text-indigo-400 hover:text-indigo-300 font-medium cursor-pointer"
                  title="Generate Retention Blueprint"
                >
                  <FileText class="w-3.5 h-3.5" />
                  <span>Blueprint</span>
                </button>

                <div class="flex items-center space-x-1.5">
                  {#if col.id === 'todo'}
                    <button
                      type="button"
                      onclick={() => moveTicket(ticket.ticket_id, 'inprogress')}
                      class="px-2 py-1 rounded bg-amber-500/10 hover:bg-amber-500/20 text-amber-300 text-[10px] font-medium transition-colors cursor-pointer"
                    >
                      Start →
                    </button>
                  {:else if col.id === 'inprogress'}
                    <button
                      type="button"
                      onclick={() => moveTicket(ticket.ticket_id, 'done')}
                      class="px-2 py-1 rounded bg-emerald-500/10 hover:bg-emerald-500/20 text-emerald-300 text-[10px] font-medium transition-colors cursor-pointer"
                    >
                      Done →
                    </button>
                  {:else if col.id === 'done'}
                    <button
                      type="button"
                      onclick={() => moveTicket(ticket.ticket_id, 'published')}
                      class="px-2 py-1 rounded bg-purple-500/10 hover:bg-purple-500/20 text-purple-300 text-[10px] font-medium transition-colors cursor-pointer"
                    >
                      Publish →
                    </button>
                  {/if}

                  <button
                    type="button"
                    onclick={() => deleteTicket(ticket.ticket_id)}
                    class="p-1 rounded text-gray-600 hover:text-rose-400 transition-colors cursor-pointer"
                    title="Delete Ticket"
                  >
                    <Trash2 class="w-3.5 h-3.5" />
                  </button>
                </div>
              </div>

            </div>
          {/each}
        </div>
      </div>
    {/each}
  </div>

  <!-- Prompt Contract Blueprint Modal -->
  {#if activePromptTicket}
    <div class="fixed inset-0 z-50 bg-black/80 backdrop-blur-sm flex items-center justify-center p-4">
      <div class="w-full max-w-2xl bg-gray-900 border border-gray-800 rounded-2xl p-6 space-y-4 shadow-2xl">
        <div class="flex items-center justify-between border-b border-gray-800 pb-3">
          <div class="flex items-center space-x-2">
            <FileText class="w-5 h-5 text-indigo-400" />
            <h3 class="text-sm font-bold text-white">
              0:00–0:45 First-Screen Retention Blueprint
            </h3>
          </div>
          <button 
            type="button"
            onclick={() => { activePromptTicket = null; activePromptText = null; }}
            class="text-gray-400 hover:text-white cursor-pointer"
          >
            <X class="w-5 h-5" />
          </button>
        </div>

        <div class="bg-gray-950 p-4 rounded-xl border border-gray-800 font-mono text-xs text-gray-300 max-h-96 overflow-y-auto whitespace-pre-wrap leading-relaxed">
          {activePromptText}
        </div>

        <div class="flex items-center justify-end space-x-2 pt-2">
          <button
            type="button"
            onclick={copyPrompt}
            class="inline-flex items-center space-x-1.5 px-4 py-2 rounded-xl bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-bold transition-all shadow-md shadow-indigo-500/20 cursor-pointer"
          >
            {#if promptCopied}
              <Check class="w-4 h-4 text-emerald-300" />
              <span>Copied to Clipboard!</span>
            {:else}
              <Copy class="w-4 h-4" />
              <span>Copy Blueprint Contract</span>
            {/if}
          </button>
        </div>
      </div>
    </div>
  {/if}

  <!-- New Ticket Modal -->
  {#if showNewModal}
    <div class="fixed inset-0 z-50 bg-black/80 backdrop-blur-sm flex items-center justify-center p-4">
      <div class="w-full max-w-md bg-gray-900 border border-gray-800 rounded-2xl p-6 space-y-4 shadow-2xl">
        <div class="flex items-center justify-between border-b border-gray-800 pb-3">
          <h3 class="text-sm font-bold text-white">
            Create Production Ticket
          </h3>
          <button 
            type="button"
            onclick={() => showNewModal = false}
            class="text-gray-400 hover:text-white cursor-pointer"
          >
            <X class="w-5 h-5" />
          </button>
        </div>

        <div class="space-y-3 text-xs">
          <div>
            <label for="ticket-title" class="block text-gray-400 mb-1 font-medium">Video Title (Zero-Colon Rule)</label>
            <input 
              id="ticket-title"
              type="text" 
              bind:value={newTitle}
              placeholder="e.g. How Linux Runs Code (Inside Syscalls & Memory Isolation)"
              class="w-full px-3 py-2 rounded-lg bg-gray-950 border border-gray-800 text-gray-100 text-xs focus:outline-none focus:border-indigo-500"
            />
          </div>

          <div>
            <label for="ticket-topic" class="block text-gray-400 mb-1 font-medium">Target Topic</label>
            <input 
              id="ticket-topic"
              type="text" 
              bind:value={newTopic}
              placeholder="e.g. Linux Kernel Syscalls"
              class="w-full px-3 py-2 rounded-lg bg-gray-950 border border-gray-800 text-gray-100 text-xs focus:outline-none focus:border-indigo-500"
            />
          </div>

          <div class="grid grid-cols-2 gap-2">
            <div>
              <label for="ticket-channel" class="block text-gray-400 mb-1 font-medium">Channel</label>
              <input 
                id="ticket-channel"
                type="text" 
                bind:value={newChannel}
                class="w-full px-3 py-2 rounded-lg bg-gray-950 border border-gray-800 text-gray-100 text-xs focus:outline-none focus:border-indigo-500"
              />
            </div>
            <div>
              <label for="ticket-duration" class="block text-gray-400 mb-1 font-medium">Duration (seconds)</label>
              <input 
                id="ticket-duration"
                type="number" 
                bind:value={newDuration}
                class="w-full px-3 py-2 rounded-lg bg-gray-950 border border-gray-800 text-gray-100 text-xs focus:outline-none focus:border-indigo-500"
              />
            </div>
          </div>
        </div>

        <div class="flex items-center justify-end space-x-2 pt-3 border-t border-gray-800">
          <button
            type="button"
            onclick={() => showNewModal = false}
            class="px-3 py-1.5 rounded-lg bg-gray-800 text-gray-300 text-xs font-medium cursor-pointer"
          >
            Cancel
          </button>
          <button
            type="button"
            onclick={createTicket}
            class="px-4 py-1.5 rounded-lg bg-indigo-600 hover:bg-indigo-500 text-white text-xs font-bold transition-all cursor-pointer"
          >
            Create Ticket
          </button>
        </div>
      </div>
    </div>
  {/if}

</div>
