import { useState, useCallback, useEffect } from 'react';
import {
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts';
import { useRpc } from '../lib/rpc';
import type { Scorecard as ScorecardType, ChannelSnapshots } from '../lib/types';
import { Trophy, TrendingUp, User } from 'lucide-react';

export default function Scorecard() {
  const { call, connected } = useRpc();
  const [data, setData] = useState<{ rows: ScorecardType[]; own_flagged: boolean } | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [snapshots, setSnapshots] = useState<ChannelSnapshots | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchScorecard = useCallback(async () => {
    if (!connected) return;
    setLoading(true);
    setError(null);
    try {
      const result = (await call('scorecard.get')) as { rows: ScorecardType[]; own_flagged: boolean; compared: number };
      setData(result);
    } catch (e) {
      setError((e as Error).message || 'Failed to load scorecard');
    } finally {
      setLoading(false);
    }
  }, [call, connected]);

  useEffect(() => {
    fetchScorecard();
  }, [fetchScorecard]);

  const fetchSnapshots = useCallback(async (channelId: string) => {
    try {
      const result = (await call('channels.snapshots', { id: channelId })) as ChannelSnapshots;
      setSnapshots(result);
    } catch (e) {
      console.warn('channels.snapshots failed:', (e as Error).message);
    }
  }, [call]);

  const handleSelect = (channelId: string) => {
    const next = selected === channelId ? null : channelId;
    setSelected(next);
    if (next) {
      fetchSnapshots(next);
    }
  };

  const rows = data?.rows ?? [];
  const ownFlagged = data?.own_flagged ?? false;

  const growthData = (snapshots?.snapshots ?? []).map((s) => ({
    at: new Date(s.at).toLocaleDateString(),
    views: s.total_views ?? 0,
    videos: s.video_count ?? 0,
  }));

  return (
    <div className="space-y-4">
      {error && (
        <div className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-400">
          {error}
        </div>
      )}

      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold">Competitor Scorecard</h1>
        <button
          onClick={fetchScorecard}
          disabled={loading || !connected}
          className="text-xs text-accent hover:underline disabled:opacity-50"
        >
          {loading ? 'Loading...' : 'Refresh'}
        </button>
      </div>

      {ownFlagged && (
        <div className="rounded-lg border border-accent/40 bg-accent/10 px-4 py-2 text-sm text-accent">
          Your channel is flagged — compare it against the competitors below.
        </div>
      )}

      {loading ? (
        <div className="text-gray-500">Loading scorecard...</div>
      ) : rows.length === 0 ? (
        <div className="rounded-xl bg-surface border border-border p-12 text-center text-gray-500">
          <Trophy size={32} className="mx-auto mb-3 opacity-50" />
          No competitors tracked yet
        </div>
      ) : (
        <>
          <div className="rounded-xl bg-surface border border-border overflow-hidden">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border">
                  <th className="px-4 py-3 text-left font-medium text-gray-400">Channel</th>
                  <th className="px-4 py-3 text-left font-medium text-gray-400">Subscribers</th>
                  <th className="px-4 py-3 text-left font-medium text-gray-400">Videos</th>
                  <th className="px-4 py-3 text-left font-medium text-gray-400">Avg Views</th>
                  <th className="px-4 py-3 text-left font-medium text-gray-400">Avg Engagement</th>
                  <th className="px-4 py-3 text-left font-medium text-gray-400">Score</th>
                  <th className="px-4 py-3 text-left font-medium text-gray-400">Centrality</th>
                  <th className="px-4 py-3" />
                </tr>
              </thead>
              <tbody>
                {rows.map((ch: ScorecardType) => (
                  <tr
                    key={ch.channel_id}
                    className={`border-b border-border/50 transition-colors cursor-pointer ${
                      ch.is_own ? 'bg-accent/10 hover:bg-accent/15' : 'hover:bg-surface-hover'
                    }`}
                    onClick={() => handleSelect(ch.channel_id)}
                  >
                    <td className="px-4 py-3 font-medium">
                      <span className="inline-flex items-center gap-1.5">
                        {ch.is_own && <User size={13} className="text-accent" />}
                        {ch.channel_name}
                        {ch.is_own && (
                          <span className="px-1.5 py-0.5 rounded-full bg-accent/20 text-accent text-[10px] font-bold">
                            YOU
                          </span>
                        )}
                      </span>
                    </td>
                    <td className="px-4 py-3 text-gray-400">
                      {ch.subscriber_count > 0 ? ch.subscriber_count.toLocaleString() : '—'}
                    </td>
                    <td className="px-4 py-3 text-gray-400">{ch.video_count}</td>
                    <td className="px-4 py-3 text-gray-400">
                      {ch.avg_views != null ? ch.avg_views.toLocaleString() : '—'}
                    </td>
                    <td className="px-4 py-3 text-gray-400">
                      {ch.avg_engagement != null ? `${(ch.avg_engagement * 100).toFixed(1)}%` : '—'}
                    </td>
                    <td className="px-4 py-3 font-bold text-accent">
                      {ch.overall_score != null ? ch.overall_score.toFixed(1) : '—'}
                    </td>
                    <td className="px-4 py-3 text-gray-400">
                      {(ch as unknown as { centrality: number | null }).centrality != null
                        ? ((ch as unknown as { centrality: number }).centrality * 100).toFixed(1)
                        : '—'}
                    </td>
                    <td className="px-4 py-3 text-right">
                      <TrendingUp
                        size={16}
                        className={`inline ${selected === ch.channel_id ? 'text-accent' : 'text-gray-600'}`}
                      />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          {selected && (
            <div className="rounded-xl bg-surface border border-border p-4">
              <h2 className="text-sm font-semibold uppercase tracking-wide text-gray-400 mb-3">
                Growth history —{' '}
                {snapshots?.channel_name ?? 'channel'} (
                {snapshots?.snapshots.length ?? 0} snapshot
                {(snapshots?.snapshots.length ?? 0) === 1 ? '' : 's'})
              </h2>
              {growthData.length < 2 ? (
                <div className="text-center text-gray-500 py-6 text-sm">
                  One snapshot recorded so far — run{' '}
                  <code className="text-gray-400">tubeforge refresh</code> a few times to see the
                  growth curve.
                </div>
              ) : (
                <ResponsiveContainer width="100%" height={220}>
                  <LineChart data={growthData} margin={{ top: 4, right: 8, bottom: 0, left: 0 }}>
                    <XAxis dataKey="at" stroke="#4a5568" fontSize={11} />
                    <YAxis
                      yAxisId="views"
                      stroke="#4f8cff"
                      fontSize={11}
                      tickFormatter={(v: number) => {
                        if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
                        if (v >= 1_000) return `${(v / 1_000).toFixed(0)}k`;
                        return String(v);
                      }}
                    />
                    <YAxis
                      yAxisId="videos"
                      orientation="right"
                      stroke="#3fbf6f"
                      fontSize={11}
                    />
                    <Tooltip
                      contentStyle={{
                        background: '#1d212b',
                        border: '1px solid #2a2f3a',
                        borderRadius: 8,
                        fontSize: 12,
                      }}
                    />
                    <Line
                      yAxisId="views"
                      type="monotone"
                      dataKey="views"
                      name="Total views"
                      stroke="#4f8cff"
                      strokeWidth={2}
                      dot={false}
                      isAnimationActive={false}
                    />
                    <Line
                      yAxisId="videos"
                      type="monotone"
                      dataKey="videos"
                      name="Videos"
                      stroke="#3fbf6f"
                      strokeWidth={2}
                      dot={false}
                      isAnimationActive={false}
                    />
                  </LineChart>
                </ResponsiveContainer>
              )}
            </div>
          )}
        </>
      )}
    </div>
  );
}
