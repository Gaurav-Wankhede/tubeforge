import { useCallback, useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { useRpc } from '../lib/rpc';
import type { OutlierVideo, CoverageTopic } from '../lib/types';
import { Flame, Target, Clock, Scissors, Loader2 } from 'lucide-react';

function outlierColor(m: number) {
  if (m >= 10) return 'text-red-400';
  if (m >= 5) return 'text-orange-400';
  return 'text-yellow-400';
}

function ScoreBadge({ score }: { score: number }) {
  const cls =
    score >= 70 ? 'bg-green-500/15 text-green-400' :
    score >= 40 ? 'bg-yellow-500/15 text-yellow-400' :
    'bg-red-500/15 text-red-400';
  return (
    <span className={`px-2 py-0.5 rounded-full text-xs font-semibold ${cls}`}>
      {score.toFixed(0)}
    </span>
  );
}

type GapOpportunity = {
  topic: string
  score: number
  demand_views: number
  channels_covering: number
  action: string
  prediction: string
}

type GraphGap = {
  topic: string
  topic_id: string
  opportunity_score: number
}

type GapsData = {
  opportunities?: GapOpportunity[]
  outliers: OutlierVideo[];
  topics: CoverageTopic[];
  freshness_gaps: string[];
  format_gaps: string[];
  graph_gaps?: GraphGap[] | null
};

export default function Gaps() {
  const { call, connected } = useRpc();
  const [data, setData] = useState<GapsData | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [progress, setProgress] = useState<string | null>(null);

  const fetchGaps = useCallback(async () => {
    if (!connected) return;
    setLoading(true);
    setError(null);
    setProgress('Starting gap analysis...');
    try {
      const result = (await call('gaps.get', {}, (_p, msg) => {
        setProgress(msg);
      })) as GapsData;
      setData(result);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
      setProgress(null);
    }
  }, [call, connected]);

  useEffect(() => {
    fetchGaps();
  }, [fetchGaps]);

  if (loading) {
    return (
      <div className="p-12 flex justify-center text-gray-500">
        <Loader2 size={20} className="animate-spin mr-2" />
        {progress || 'Computing gap report...'}
      </div>
    );
  }

  if (error || !data) {
    return (
      <div className="p-12 text-center text-gray-500">
        {error || 'Could not load the gap report. Run <code className="text-gray-400">tubeforge ingest</code> first.'}
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold">Competitor Gap Mining</h1>
        <button
          onClick={fetchGaps}
          disabled={loading || !connected}
          className="text-xs text-accent hover:underline disabled:opacity-50"
        >
          {loading ? 'Analyzing...' : 'Refresh'}
        </button>
      </div>

      {/* Actionable: topics YOU should win */}
      {data.opportunities && data.opportunities.length > 0 && (
        <div className="rounded-xl bg-surface border border-accent/30 p-4">
          <h2 className="text-sm font-semibold text-accent mb-3">
            Topics you should win — pick your next video here
          </h2>
          <div className="space-y-2">
            {data.opportunities.map((o) => (
              <div key={o.topic} className="flex items-center gap-3 rounded-lg bg-gray-800/30 border border-border p-3">
                <div className="flex-1 min-w-0">
                  <div className="text-sm font-medium text-gray-100">{o.topic}</div>
                  <div className="text-xs text-gray-400 mt-0.5">{o.action}</div>
                </div>
                <div className="text-right shrink-0">
                  <div className={`text-xs font-bold ${o.prediction === 'Very High' ? 'text-green-400' : 'text-yellow-400'}`}>
                    {o.prediction}
                  </div>
                  <div className="text-[11px] text-gray-500">{o.channels_covering} channel(s) cover it</div>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Outliers — Method A */}
      <section>
        <div className="flex items-center gap-2 mb-3">
          <Flame size={16} className="text-orange-400" />
          <h2 className="text-sm font-semibold uppercase tracking-wide text-gray-400">
            Outliers — proven demand ({data.outliers.length})
          </h2>
        </div>
        {data.outliers.length === 0 ? (
          <div className="rounded-xl bg-surface border border-border p-6 text-center text-gray-500 text-sm">
            No videos at ≥3× their channel's mean views.
          </div>
        ) : (
          <div className="rounded-xl bg-surface border border-border overflow-hidden">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border">
                  <th className="px-4 py-3 text-left font-medium text-gray-400">Video</th>
                  <th className="px-4 py-3 text-left font-medium text-gray-400">Channel</th>
                  <th className="px-4 py-3 text-right font-medium text-gray-400">Views</th>
                  <th className="px-4 py-3 text-right font-medium text-gray-400">×mean</th>
                </tr>
              </thead>
              <tbody>
                {data.outliers.map((o: OutlierVideo) => (
                  <tr key={o.video_id} className="border-b border-border/50 hover:bg-surface-hover">
                    <td className="px-4 py-3">
                      <Link to={`/scores/${o.video_id}`} className="hover:text-accent">
                        {o.title}
                      </Link>
                    </td>
                    <td className="px-4 py-3 text-gray-400">{o.channel_name}</td>
                    <td className="px-4 py-3 text-right text-gray-400">
                      {o.views != null ? o.views.toLocaleString() : '—'}
                    </td>
                    <td className={`px-4 py-3 text-right font-bold ${outlierColor(o.multiple)}`}>
                      {o.multiple != null ? `${o.multiple.toFixed(1)}×` : '—'}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      {/* Coverage map — Method C */}
      <section>
        <div className="flex items-center gap-2 mb-3">
          <Target size={16} className="text-accent" />
          <h2 className="text-sm font-semibold uppercase tracking-wide text-gray-400">
            Coverage map — topic gaps ({data.topics.length})
          </h2>
        </div>
        {data.topics.length === 0 ? (
          <div className="rounded-xl bg-surface border border-border p-6 text-center text-gray-500 text-sm">
            No topics found — ingest competitor channels first.
          </div>
        ) : (
          <div className="rounded-xl bg-surface border border-border overflow-hidden">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border">
                  <th className="px-4 py-3 text-left font-medium text-gray-400">Topic</th>
                  <th className="px-4 py-3 text-right font-medium text-gray-400">Videos</th>
                  <th className="px-4 py-3 text-right font-medium text-gray-400">Channels</th>
                  <th className="px-4 py-3 text-right font-medium text-gray-400">Mean views</th>
                  <th className="px-4 py-3 text-center font-medium text-gray-400">Short?</th>
                  <th className="px-4 py-3 text-center font-medium text-gray-400">Series?</th>
                  <th className="px-4 py-3 text-right font-medium text-gray-400">Gap</th>
                </tr>
              </thead>
              <tbody>
                {data.topics.map((t: CoverageTopic) => (
                  <tr key={t.topic} className="border-b border-border/50 hover:bg-surface-hover">
                    <td className="px-4 py-3 font-medium">{t.topic}</td>
                    <td className="px-4 py-3 text-right text-gray-400">{t.videos}</td>
                    <td className="px-4 py-3 text-right text-gray-400">{t.channels}</td>
                    <td className="px-4 py-3 text-right text-gray-400">
                      {t.mean_views > 0 ? t.mean_views.toLocaleString() : '—'}
                    </td>
                    <td className="px-4 py-3 text-center">
                      <span className={t.no_short ? 'text-orange-400' : 'text-green-400'}>
                        {t.no_short ? 'no' : 'yes'}
                      </span>
                    </td>
                    <td className="px-4 py-3 text-center">
                      {t.is_series ? <span className="text-accent">✓</span> : <span className="text-gray-600">—</span>}
                    </td>
                    <td className="px-4 py-3 text-right">
                      <ScoreBadge score={t.score} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      {/* Freshness + format gaps */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <section className="rounded-xl bg-surface border border-border p-4">
          <div className="flex items-center gap-2 mb-3">
            <Clock size={16} className="text-yellow-400" />
            <h2 className="text-sm font-semibold uppercase tracking-wide text-gray-400">
              Freshness gaps ({data.freshness_gaps.length})
            </h2>
          </div>
          {data.freshness_gaps.length === 0 ? (
            <p className="text-sm text-gray-500">No stale topics.</p>
          ) : (
            <ul className="space-y-1.5">
              {data.freshness_gaps.map((t) => (
                <li key={t} className="text-sm text-gray-300">
                  · {t}
                </li>
              ))}
            </ul>
          )}
        </section>
        <section className="rounded-xl bg-surface border border-border p-4">
          <div className="flex items-center gap-2 mb-3">
            <Scissors size={16} className="text-green-400" />
            <h2 className="text-sm font-semibold uppercase tracking-wide text-gray-400">
              Format gaps — no Short version ({data.format_gaps.length})
            </h2>
          </div>
          {data.format_gaps.length === 0 ? (
            <p className="text-sm text-gray-500">No Short gaps.</p>
          ) : (
            <ul className="space-y-1.5">
              {data.format_gaps.map((t) => (
                <li key={t} className="text-sm text-gray-300">
                  · {t}
                </li>
              ))}
            </ul>
          )}
        </section>

        {/* Graph-based gaps (from Knowledge Graph — when available) */}
        {data.graph_gaps && data.graph_gaps.length > 0 && (
          <section className="rounded-xl bg-surface border border-accent/30 p-4">
            <div className="flex items-center gap-2 mb-3">
              <Target size={16} className="text-accent" />
              <h2 className="text-sm font-semibold uppercase tracking-wide text-gray-400">
                Knowledge Graph gaps ({data.graph_gaps.length})
              </h2>
            </div>
            <div className="space-y-2">
              {data.graph_gaps.map((g) => (
                <div key={g.topic_id} className="flex items-center justify-between rounded-lg bg-gray-800/30 border border-border p-3">
                  <div className="flex-1 min-w-0">
                    <div className="text-sm font-medium text-gray-100">{g.topic}</div>
                    <div className="text-xs text-gray-500 mt-0.5">High demand, low supply</div>
                  </div>
                  <ScoreBadge score={g.opportunity_score} />
                </div>
              ))}
            </div>
            <p className="text-xs text-gray-600 mt-2">Detected via community analysis. Run <code>tubeforge kg build</code> to update.</p>
          </section>
        )}
      </div>
    </div>
  );
}
