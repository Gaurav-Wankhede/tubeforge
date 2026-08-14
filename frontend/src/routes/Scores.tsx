import { useState, useCallback, useEffect } from 'react';
import { Link } from 'react-router-dom';
import { useRpc } from '../lib/rpc';
import type { ScoreRow } from '../lib/types';
import { Trophy, Search, Loader2, RefreshCw } from 'lucide-react';

type ScoresResult = {
  scores: ScoreRow[];
  total: number;
};

export default function Scores() {
  const { call, connected } = useRpc();
  const [data, setData] = useState<ScoresResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [backfilling, setBackfilling] = useState(false);
  const [backfillMsg, setBackfillMsg] = useState('');
  const [q, setQ] = useState('');

  const fetchScores = useCallback(async () => {
    if (!connected) return;
    setLoading(true);
    setError(null);
    try {
      const result = (await call('scores.list', { q })) as ScoresResult;
      setData(result);
    } catch (e) {
      setError((e as Error).message || 'Failed to load scores');
    } finally {
      setLoading(false);
    }
  }, [call, connected, q]);

  useEffect(() => {
    fetchScores();
  }, [fetchScores]);

  // Score any videos lacking a stored score — one-time backfill so the list
  // reflects fresh analysis for every collected video.
  const runBackfill = useCallback(async () => {
    if (!connected) return;
    setBackfilling(true);
    setBackfillMsg('Starting backfill...');
    try {
      await call('scores.backfill', {}, (_p, msg) => setBackfillMsg(msg));
      await fetchScores();
    } catch (e) {
      setBackfillMsg((e as Error).message);
    } finally {
      setBackfilling(false);
    }
  }, [call, connected, fetchScores]);

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold">Scores</h1>
        <div className="flex items-center gap-2">
          <button
            onClick={runBackfill}
            disabled={loading || backfilling || !connected}
            className="flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-lg bg-accent/10 border border-accent/25 text-accent hover:bg-accent/20 disabled:opacity-50 transition-colors"
          >
            <RefreshCw size={12} className={backfilling ? 'animate-spin' : ''} />
            {backfilling ? 'Scoring...' : 'Score missing'}
          </button>
          <button
            onClick={fetchScores}
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

      {backfilling && backfillMsg && (
        <div className="flex items-center gap-2 text-xs text-accent bg-accent/5 border border-accent/20 rounded-lg px-3 py-2">
          <RefreshCw size={12} className="animate-spin" />
          {backfillMsg}
        </div>
      )}

      {/* Search */}
      <div className="relative">
        <Search size={16} className="absolute left-3 top-1/2 -translate-y-1/2 text-gray-500" />
        <input
          type="text"
          placeholder="Filter by title..."
          value={q}
          onChange={(e) => setQ(e.target.value)}
          className="w-full pl-9 pr-3 py-2.5 bg-surface border border-border rounded-lg text-sm focus:outline-none focus:border-accent"
        />
      </div>

      {loading ? (
        <div className="flex justify-center py-8 text-gray-500">
          <Loader2 size={20} className="animate-spin mr-2" /> Loading scores...
        </div>
      ) : !data || data.scores.length === 0 ? (
        <div className="rounded-xl bg-surface border border-border p-12 text-center text-gray-500">
          <Trophy size={32} className="mx-auto mb-3 opacity-50" />
          No scored videos — run <code className="text-gray-400">tubeforge score</code>.
        </div>
      ) : (
        <div className="rounded-xl bg-surface border border-border overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-border">
                <th className="px-4 py-3 text-left font-medium text-gray-400">Title</th>
                <th className="px-4 py-3 text-left font-medium text-gray-400">Channel</th>
                <th className="px-4 py-3 text-right font-medium text-gray-400">SEO</th>
                <th className="px-4 py-3 text-right font-medium text-gray-400">Freshness</th>
                <th className="px-4 py-3 text-right font-medium text-gray-400">Authority</th>
              </tr>
            </thead>
            <tbody>
              {data.scores.map((s: ScoreRow) => (
                <tr key={s.video_id} className="border-b border-border/50 hover:bg-surface-hover">
                  <td className="px-4 py-3">
                    <Link to={`/scores/${s.video_id}`} className="hover:text-accent">
                      {s.title}
                    </Link>
                  </td>
                  <td className="px-4 py-3 text-gray-400">{s.channel_name}</td>
                  <td className="px-4 py-3 text-right font-bold text-accent">
                    {s.overall_score != null ? s.overall_score.toFixed(1) : '—'}
                  </td>
                  <td className="px-4 py-3 text-right text-gray-400">
                    {s.freshness_score != null ? s.freshness_score.toFixed(1) : '—'}
                  </td>
                  <td className="px-4 py-3 text-right text-gray-400">
                    {s.authority_score != null ? s.authority_score.toFixed(1) : '—'}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
