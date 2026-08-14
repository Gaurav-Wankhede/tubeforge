import { useEffect, useState, useCallback } from 'react';
import { TrendingUp, Users, Film, Tags, Lightbulb, Bell, Key } from 'lucide-react';
import { useRpc } from '../lib/rpc';

const iconMap: Record<string, typeof TrendingUp> = {
  videos: Film,
  channels: Users,
  tags: Tags,
  ideas: Lightbulb,
  alerts: Bell,
  keywords: Key,
};

const colorMap: Record<string, string> = {
  videos: 'from-blue-500 to-blue-600',
  channels: 'from-purple-500 to-purple-600',
  tags: 'from-green-500 to-green-600',
  ideas: 'from-yellow-500 to-yellow-600',
  alerts: 'from-red-500 to-red-600',
  keywords: 'from-cyan-500 to-cyan-600',
};

export default function Dashboard() {
  const { call, connected } = useRpc();
  const [dashboard, setDashboard] = useState<Record<string, unknown> | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchDashboard = useCallback(async () => {
    if (!connected) return;
    setLoading(true);
    setError(null);
    try {
      const result = (await call('dashboard.overview')) as Record<string, unknown>;
      setDashboard(result);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }, [call, connected]);

  useEffect(() => {
    fetchDashboard();
  }, [fetchDashboard]);

  const counts = (dashboard?.counts as Record<string, number>) ?? {};

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold">Dashboard</h1>
        <button
          onClick={fetchDashboard}
          disabled={loading || !connected}
          className="text-xs text-accent hover:underline disabled:opacity-50"
        >
          {loading ? 'Loading...' : 'Refresh'}
        </button>
      </div>

      {error && <div className="text-red-400 text-sm">{error}</div>}

      {/* Counter cards */}
      <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-6 gap-3">
        {Object.entries(counts).map(([key, value]) => {
          const Icon = iconMap[key] || TrendingUp;
          const grad = colorMap[key] || 'from-gray-500 to-gray-600';
          return (
            <div
              key={key}
              className="rounded-xl bg-surface border border-border p-4 hover:border-accent/50 transition-colors"
            >
              <div className="flex items-center gap-2 mb-2">
                <div className={`p-1.5 rounded-lg bg-gradient-to-br ${grad}`}>
                  <Icon size={14} className="text-white" />
                </div>
                <span className="text-xs text-gray-400 capitalize">{key}</span>
              </div>
              <div className="text-2xl font-bold">
                {loading ? '—' : (value as number).toLocaleString()}
              </div>
            </div>
          );
        })}
      </div>

      {/* Health & quota */}
      {dashboard && (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div className="rounded-xl bg-surface border border-border p-4">
            <h2 className="text-sm font-medium text-gray-400 mb-2">Integrity</h2>
            <div className="text-sm">{((dashboard.integrity as string) || '—') as string}</div>
          </div>
          <div className="rounded-xl bg-surface border border-border p-4">
            <h2 className="text-sm font-medium text-gray-400 mb-2">Quota</h2>
            <div className="text-sm">
              {(() => {
                const q = dashboard.quota as { videos_list_used?: number; daily_limit?: number };
                return `${q?.videos_list_used ?? 0} / ${q?.daily_limit ?? 0} videos.list units`;
              })()}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
