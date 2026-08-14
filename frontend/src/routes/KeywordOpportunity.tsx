import { useCallback, useEffect, useState } from 'react';
import {
  Line,
  LineChart,
  ResponsiveContainer,
} from 'recharts';
import { useRpc } from '../lib/rpc';
import type { AnalysisKeywords, KeywordOpportunity } from '../lib/types';
import { Key, Loader2 } from 'lucide-react';
import { FreshnessBadge } from '../components/FreshnessBadge';

export default function KeywordOpportunity() {
  const { call, connected } = useRpc();
  const [data, setData] = useState<AnalysisKeywords | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [horizon, setHorizon] = useState(7);

  const fetchOpportunities = useCallback(async () => {
    if (!connected) return;
    setLoading(true);
    setError(null);
    try {
      const result = (await call('analysis.keywords', { horizon })) as AnalysisKeywords;
      setData(result);
    } catch (e) {
      setError((e as Error).message || 'Failed to load keyword opportunities');
    } finally {
      setLoading(false);
    }
  }, [call, connected, horizon]);

  useEffect(() => {
    fetchOpportunities();
  }, [fetchOpportunities]);

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h1 className="text-2xl font-bold">Keyword Opportunity</h1>
          <FreshnessBadge at={data?.research_at ?? null} />
        </div>
        <div className="flex items-center gap-3">
          <select
            value={horizon}
            onChange={(e) => setHorizon(Number(e.target.value))}
            className="text-xs bg-surface border border-border rounded px-2 py-1"
          >
            <option value={7}>7 days</option>
            <option value={14}>14 days</option>
            <option value={30}>30 days</option>
          </select>
          <button
            onClick={fetchOpportunities}
            disabled={loading || !connected}
            className="text-xs text-accent hover:underline disabled:opacity-50"
          >
            {loading ? 'Loading...' : 'Refresh'}
          </button>
        </div>
      </div>

      {error && (
        <div className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-400">
          {error}
        </div>
      )}

      {loading ? (
        <div className="flex justify-center py-8 text-gray-500">
          <Loader2 size={20} className="animate-spin mr-2" /> Analyzing keywords...
        </div>
      ) : !data || data.opportunities.length === 0 ? (
        <div className="rounded-xl bg-surface border border-border p-12 text-center text-gray-500">
          <Key size={32} className="mx-auto mb-3 opacity-50" />
          No keyword opportunities found.
        </div>
      ) : (
        <>
          <div className="rounded-xl bg-surface border border-border overflow-hidden">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border">
                  <th className="px-4 py-3 text-left font-medium text-gray-400">Keyword</th>
                  <th className="px-4 py-3 text-right font-medium text-gray-400">Opportunity</th>
                  <th className="px-4 py-3 text-right font-medium text-gray-400">Competition</th>
                  <th className="px-4 py-3 text-left font-medium text-gray-400">Volume</th>
                  <th className="px-4 py-3 text-left font-medium text-gray-400">Trend</th>
                </tr>
              </thead>
              <tbody>
                {data.opportunities.map((o: KeywordOpportunity) => (
                  <tr key={o.keyword} className="border-b border-border/50 hover:bg-surface-hover">
                    <td className="px-4 py-3 font-medium">{o.keyword}</td>
                    <td className="px-4 py-3 text-right font-bold text-green-400">{o.opportunity.toFixed(0)}</td>
                    <td className="px-4 py-3 text-right text-gray-400">{o.competition.toFixed(0)}</td>
                    <td className="px-4 py-3 text-gray-400">{o.volume}</td>
                    <td className="px-4 py-3">
                      {o.trend.length > 1 ? (
                        <ResponsiveContainer width={80} height={30}>
                          <LineChart data={o.trend.map((p) => ({ v: p.value }))}>
                            <Line type="monotone" dataKey="v" stroke="#4f8cff" strokeWidth={1.5} dot={false} isAnimationActive={false} />
                          </LineChart>
                        </ResponsiveContainer>
                      ) : (
                        <span className="text-gray-600">—</span>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="text-xs text-gray-500">
            Horizon: {data.horizon_days} days · {data.opportunities.length} opportunities
          </div>
        </>
      )}
    </div>
  );
}
