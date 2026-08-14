import { useCallback, useEffect, useState } from 'react';
import { useRpc } from '../lib/rpc';
import type { TagCloud, TagGap } from '../lib/types';
import { Tags as TagsIcon, TrendingUp, TrendingDown, Minus, Loader2 } from 'lucide-react';

const trendIcon = (t: string) => {
  if (t === 'rising') return <TrendingUp size={12} className="text-green-400" />;
  if (t === 'declining') return <TrendingDown size={12} className="text-red-400" />;
  return <Minus size={12} className="text-gray-400" />;
};

type TagIntelData = {
  cloud: TagCloud | null;
  gaps: TagGap[];
};

export default function TagIntelligence() {
  const { call, connected } = useRpc();
  const [data, setData] = useState<TagIntelData>({ cloud: null, gaps: [] });
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchData = useCallback(async () => {
    if (!connected) return;
    setLoading(true);
    setError(null);
    try {
      const [cloud, gapsRes] = await Promise.all([
        call('tags.cloud') as Promise<TagCloud>,
        call('tags.gaps') as Promise<{ gaps: TagGap[] }>,
      ]);
      setData({ cloud, gaps: gapsRes.gaps });
    } catch (e) {
      setError((e as Error).message || 'Failed to load tag intelligence');
    } finally {
      setLoading(false);
    }
  }, [call, connected]);

  useEffect(() => {
    fetchData();
  }, [fetchData]);

  if (loading) {
    return (
      <div className="p-12 flex justify-center text-gray-500">
        <Loader2 size={20} className="animate-spin mr-2" /> Analyzing tags...
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <TagsIcon size={20} className="text-accent" />
          <h1 className="text-2xl font-bold">Tag Intelligence</h1>
        </div>
        <button
          onClick={fetchData}
          disabled={loading || !connected}
          className="text-xs text-accent hover:underline disabled:opacity-50"
        >
          {loading ? 'Loading...' : 'Refresh'}
        </button>
      </div>

      {error && (
        <div className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-400">
          {error}
        </div>
      )}

      {/* Tag cloud */}
      <section className="rounded-xl bg-surface border border-border p-4">
        <h2 className="text-sm font-medium text-gray-400 mb-3">
          Tag cloud ({data.cloud?.total_unique ?? 0} unique)
        </h2>
        {data.cloud?.tags.length === 0 ? (
          <p className="text-sm text-gray-500">No tags yet.</p>
        ) : (
          <div className="flex flex-wrap gap-2">
            {data.cloud?.tags.map((t) => (
              <span
                key={t.name}
                className="inline-flex items-center gap-1 px-2.5 py-1 rounded-full bg-accent/10 border border-accent/25 text-xs text-accent"
              >
                {t.name}
                <span className="text-[10px] opacity-70">×{t.count}</span>
                {trendIcon(t.trend)}
              </span>
            ))}
          </div>
        )}
      </section>

      {/* Tag gaps */}
      <section className="rounded-xl bg-surface border border-border p-4">
        <h2 className="text-sm font-medium text-gray-400 mb-3">
          Tag gaps — competitors use, you don&apos;t
        </h2>
        {data.gaps.length === 0 ? (
          <p className="text-sm text-gray-500">No tag gaps found.</p>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border">
                  <th className="px-4 py-3 text-left font-medium text-gray-400">Tag</th>
                  <th className="px-4 py-3 text-right font-medium text-gray-400">Competitor usage</th>
                  <th className="px-4 py-3 text-right font-medium text-gray-400">Your usage</th>
                  <th className="px-4 py-3 text-right font-medium text-gray-400">Opportunity</th>
                </tr>
              </thead>
              <tbody>
                {data.gaps.map((g) => (
                  <tr key={g.tag} className="border-b border-border/50 hover:bg-surface-hover">
                    <td className="px-4 py-3 font-medium">{g.tag}</td>
                    <td className="px-4 py-3 text-right text-gray-400">{g.competitor_usage}</td>
                    <td className="px-4 py-3 text-right text-gray-400">{g.your_usage}</td>
                    <td className="px-4 py-3 text-right font-bold text-accent">
                      {g.opportunity_score.toFixed(0)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </div>
  );
}
