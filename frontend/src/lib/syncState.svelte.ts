export interface SyncStatus {
  is_running: boolean;
  total: number;
  processed: number;
  tags_synced: number;
  current_title: string;
  started_at: string | null;
  finished_at: string | null;
  message: string;
}

class SyncManager {
  status = $state<SyncStatus>({
    is_running: false,
    total: 0,
    processed: 0,
    tags_synced: 0,
    current_title: '',
    started_at: null,
    finished_at: null,
    message: 'Idle',
  });

  private pollInterval: any = null;

  constructor() {
    this.fetchStatus();
  }

  async fetchStatus() {
    try {
      const res = await fetch('/api/sync/status');
      if (res.ok) {
        const data: SyncStatus = await res.json();
        this.status = data;

        // If the backend is running, ensure polling is active
        if (data.is_running && !this.pollInterval) {
          this.startPolling();
        } else if (!data.is_running && this.pollInterval) {
          // As soon as the task completes, STOP ALL POLLING completely
          this.stopPolling();
        }
      }
    } catch {
      this.stopPolling();
    }
  }

  startPolling() {
    if (this.pollInterval) return;
    this.pollInterval = setInterval(() => {
      this.fetchStatus();
    }, 1500);
  }

  stopPolling() {
    if (this.pollInterval) {
      clearInterval(this.pollInterval);
      this.pollInterval = null;
    }
  }

  async triggerSync() {
    if (this.status.is_running) return;
    try {
      this.status.is_running = true;
      this.status.message = 'Starting live YouTube sync...';
      this.startPolling();
      await fetch('/api/sync', { method: 'POST' });
      await this.fetchStatus();
    } catch (e) {
      console.error('Failed to trigger live sync:', e);
      this.stopPolling();
    }
  }
}

export const syncManager = new SyncManager();
