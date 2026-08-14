import { useCallback, useEffect, useState } from 'react';
import { useRpc } from '../lib/rpc';
import type { HealthReport } from '../lib/types';
import {
  Database,
  Server,
  CheckCircle,
  AlertTriangle,
  Clock,
  Film,
  Users,
  Key,
  Lightbulb,
  Bell,
  RefreshCw,
} from 'lucide-react';

const countIcons: Record<string, typeof Film> = {
  channels: Users,
  videos: Film,
  ideas: Lightbulb,
  alerts: Bell,
  keywords: Key,
};

export default function Health() {
  const { call, connected } = useRpc();
  const [data, setData] = useState<HealthReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchHealth = useCallback(async () => {
    if (!connected) return;
    setLoading(true);
    setError(null);
    try {
      const result = (await call('health.get')) as HealthReport;
      setData(result);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }, [call, connected]);

  useEffect(() => {
    fetchHealth();
  }, [fetchHealth]);

  if (loading) {
    return <div className="text-gray-500">Loading health report...</div>;
  }

  if (error || !data) {
    return <div className="text-gray-500">{error || 'Failed to load health report'}</div>;
  }

  const h: HealthReport = data;
  const integrityOk = h.integrity === 'ok';

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold">System Health</h1>
        <button
          onClick={fetchHealth}
          disabled={loading || !connected}
          className="text-xs text-accent hover:underline disabled:opacity-50"
        >
          {loading ? 'Loading...' : 'Refresh'}
        </button>
      </div>

      {/* Core status cards */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <div className="rounded-xl bg-surface border border-border p-4">
          <div className="flex items-center gap-3">
            <CheckCircle size={18} className={integrityOk ? 'text-green-400' : 'text-red-400'} />
            <div>
              <div className="font-medium">Database integrity</div>
              <div className={`text-xs ${integrityOk ? 'text-gray-400' : 'text-red-400'}`}>
                {integrityOk ? 'integrity_check passed' : h.integrity}
              </div>
            </div>
          </div>
        </div>
        <div className="rounded-xl bg-surface border border-border p-4">
          <div className="flex items-center gap-3">
            <RefreshCw size={18} className={h.index.fresh ? 'text-green-400' : 'text-yellow-400'} />
            <div>
              <div className="font-medium">Search index</div>
              <div className="text-xs text-gray-400">
                {h.index.fresh ? 'fresh' : 'stale — run tubeforge reindex'}
              </div>
            </div>
          </div>
        </div>
        <div className="rounded-xl bg-surface border border-border p-4">
          <div className="flex items-center gap-3">
            <Server size={18} className="text-accent" />
            <div>
              <div className="font-medium">API quota</div>
              <div className="text-xs text-gray-400">
                {h.quota.videos_list_used} / {h.quota.daily_limit} used · {h.quota.date}
              </div>
            </div>
          </div>
        </div>
      </div>

      {/* Counts */}
      <div className="rounded-xl bg-surface border border-border p-4">
        <div className="flex items-center gap-2 mb-3">
          <Database size={14} className="text-gray-400" />
          <h2 className="text-sm font-medium text-gray-400">Corpus counts</h2>
        </div>
        <div className="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-6 gap-3">
          {Object.entries(h.counts).map(([key, value]) => {
            const Icon = countIcons[key] || Database;
            return (
              <div key={key} className="rounded-lg bg-gray-800/40 border border-border p-3">
                <div className="flex items-center gap-1.5 text-[11px] text-gray-500 mb-1">
                  <Icon size={11} />
                  <span className="capitalize">{key.replace('_', ' ')}</span>
                </div>
                <div className="text-lg font-bold">{value.toLocaleString()}</div>
              </div>
            );
          })}
        </div>
      </div>

      {/* Stale channels */}
      <div className="rounded-xl bg-surface border border-border p-4">
        <div className="flex items-center gap-2 mb-3">
          <Clock size={14} className="text-yellow-400" />
          <h2 className="text-sm font-medium text-gray-400">
            Stale channels ({h.stale_channels.length}) — not fetched in {h.stale_days} days
          </h2>
        </div>
        {h.stale_channels.length === 0 ? (
          <p className="text-sm text-gray-500">All channels are fresh. Run{' '}
            <code className="text-gray-400">tubeforge refresh</code> to keep them that way.</p>
        ) : (
          <ul className="space-y-1.5">
            {h.stale_channels.map((c) => (
              <li key={c.channel_id} className="flex items-center gap-2 text-sm">
                <AlertTriangle size={12} className="text-yellow-400" />
                <span className="text-gray-300">{c.title}</span>
                <span className="text-gray-500 text-xs">fetched {c.fetched_at}</span>
              </li>
            ))}
          </ul>
        )}
      </div>

      {/* Privacy + metadata completeness */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <div className="rounded-xl bg-surface border border-border p-4">
          <h2 className="text-sm font-medium text-gray-400 mb-3">Privacy status</h2>
          <div className="flex gap-6 text-sm">
            <div>
              <span className="text-gray-500">Unlisted</span>
              <p className="font-medium">{h.privacy.unlisted}</p>
            </div>
            <div>
              <span className="text-gray-500">Private</span>
              <p className="font-medium">{h.privacy.private}</p>
            </div>
          </div>
        </div>
        <div className="rounded-xl bg-surface border border-border p-4">
          <h2 className="text-sm font-medium text-gray-400 mb-3">Metadata completeness</h2>
          <div className="text-sm">
            <div className="flex justify-between">
              <span className="text-gray-500">Engagement completeness</span>
              <span className="font-medium">{(h.metadata_completeness.engagement_complete * 100).toFixed(0)}%</span>
            </div>
            <div className="text-xs text-gray-500 mt-2">
              {h.metadata_completeness.disabled_metrics.videos} videos have disabled metrics
              (views {h.metadata_completeness.disabled_metrics.view_count} · likes{' '}
              {h.metadata_completeness.disabled_metrics.like_count} · comments{' '}
              {h.metadata_completeness.disabled_metrics.comment_count})
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
