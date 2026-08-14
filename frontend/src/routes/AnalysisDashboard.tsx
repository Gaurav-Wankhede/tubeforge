import { useCallback, useEffect, useState } from 'react';
import {
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
  CartesianGrid,
} from 'recharts';
import { useRpc } from '../lib/rpc';
import type { AnalysisOverview, TagGapInsight } from '../lib/types';
import { Users, Film, Eye, TrendingUp, Trophy, Loader2, Sparkles, RefreshCw, Database, Network } from 'lucide-react';

function StatCard({ label, value, hint, icon: Icon }: { label: string; value: string; hint?: string; icon: typeof Users }) {
  return (
    <div className="rounded-xl bg-surface border border-border p-4">
      <div className="flex items-center gap-2 mb-1">
        <Icon size={14} className="text-accent" />
        <span className="text-xs uppercase tracking-wide text-gray-500">{label}</span>
      </div>
      <div className="text-2xl font-bold">{value}</div>
      {hint && <div className="text-xs text-gray-500">{hint}</div>}
    </div>
  );
}

function fmtViews(v: number) {
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
  if (v >= 1_000) return `${(v / 1_000).toFixed(1)}k`;
  return String(v);
}

export default function AnalysisDashboard() {
  const { call, connected } = useRpc();
  const [data, setData] = useState<AnalysisOverview | null>(null);
  const [loading, setLoading] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshMsg, setRefreshMsg] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [graphSvg, setGraphSvg] = useState<string | null>(null);
  const [graphError, setGraphError] = useState<string | null>(null);

  const fetchOverview = useCallback(async () => {
    if (!connected) return;
    setLoading(true);
    setError(null);
    try {
      const result = (await call('analysis.overview')) as AnalysisOverview;
      setData(result);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }, [call, connected]);

  useEffect(() => {
    fetchOverview();
  }, [fetchOverview]);

  // One click → pull LIVE YouTube data for tracked keywords, persist to the
  // DB, then re-render the refreshed overview. Runtime sync, on demand.
  const fetchLive = useCallback(async () => {
    if (!connected) return;
    setRefreshing(true);
    setRefreshMsg('Fetching live YouTube data...');
    setError(null);
    try {
      const result = (await call('analysis.refresh', {}, (_p, msg) => setRefreshMsg(msg), 300_000)) as {
        overview: AnalysisOverview | null;
        refreshed: number;
        subscribers_updated: number;
        message: string;
      };
      if (result.overview) setData(result.overview);
      setRefreshMsg(
        result.message ||
          `Refreshed ${result.refreshed} keyword(s), ${result.subscribers_updated ?? 0} subscriber count(s)`,
      );
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setRefreshing(false);
    }
  }, [call, connected]);

  // Fetch graph SVG on mount (when connected)
  useEffect(() => {
    if (!connected) return;
    fetch('/api/analysis/graph')
      .then((res) => {
        if (!res.ok) throw new Error('Knowledge Graph not built yet');
        return res.text();
      })
      .then((svg) => setGraphSvg(svg))
      .catch((e) => setGraphError((e as Error).message));
  }, [connected]);

  if (loading) {
    return (
      <div className="p-12 flex justify-center text-gray-500">
        <Loader2 size={20} className="animate-spin mr-2" /> Analyzing your channel...
      </div>
    );
  }

  if (error || !data) {
    return (
      <div className="p-12 text-center text-gray-500">
        <TrendingUp size={32} className="mx-auto mb-3 opacity-50" />
        {error || (
          <>
            Set <code className="text-accent">TUBEFORGE_OWN_CHANNEL</code> in .env to analyze your
            channel&apos;s growth.
          </>
        )}
      </div>
    );
  }

  const growthData = data.growth.map((p) => ({ label: p.label, views: p.value }));

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Sparkles size={20} className="text-accent" />
          <h1 className="text-2xl font-bold">Growth Command Center</h1>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={fetchLive}
            disabled={refreshing || !connected}
            className="flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-lg bg-accent/15 border border-accent/30 text-accent hover:bg-accent/25 disabled:opacity-50 transition-colors"
            title="Pull live YouTube data for tracked keywords and persist to the database"
          >
            <RefreshCw size={12} className={refreshing ? 'animate-spin' : ''} />
            {refreshing ? 'Fetching...' : 'Fetch Live YouTube'}
          </button>
          <button
            onClick={fetchOverview}
            disabled={loading || !connected}
            className="text-xs text-accent hover:underline disabled:opacity-50"
          >
            {loading ? 'Analyzing...' : 'Refresh'}
          </button>
        </div>
      </div>

      {(refreshing || refreshMsg) && (
        <div className="flex items-center gap-2 text-xs text-accent bg-accent/5 border border-accent/20 rounded-lg px-3 py-2">
          <Database size={12} />
          {refreshMsg || 'Fetching live YouTube data...'}
        </div>
      )}

      {/* Stat cards */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
        <StatCard label="Subscribers" value={data.subscriber_count.toLocaleString()} icon={Users} hint={`vs competitor median ${fmtViews(data.competitor.subscriber_median)}`} />
        <StatCard label="Videos" value={String(data.video_count)} icon={Film} />
        <StatCard label="Total views" value={fmtViews(data.total_views)} icon={Eye} hint={`avg ${fmtViews(data.avg_views)}/video`} />
        <StatCard label="Avg SEO score" value={data.avg_score.toFixed(0)} icon={TrendingUp} hint={`vs competitor median ${data.competitor.score_median.toFixed(0)}`} />
      </div>

      {/* Growth chart */}
      <div className="rounded-xl bg-surface border border-border p-4">
        <h2 className="text-sm font-medium text-gray-400 mb-3">Your channel growth (total views over time)</h2>
        <ResponsiveContainer width="100%" height={220}>
          <LineChart data={growthData} margin={{ top: 4, right: 8, bottom: 0, left: 0 }}>
            <CartesianGrid stroke="#2a2f3a" vertical={false} />
            <XAxis dataKey="label" tick={{ fill: '#9ca3af', fontSize: 11 }} />
            <YAxis tick={{ fill: '#9ca3af', fontSize: 11 }} tickFormatter={(v: number) => fmtViews(v)} />
            <Tooltip contentStyle={{ background: '#1a1a1a', border: '1px solid #333', borderRadius: 8, color: '#fff' }} />
            <Line type="monotone" dataKey="views" stroke="#4f8cff" strokeWidth={2} dot={{ r: 3 }} isAnimationActive={false} />
          </LineChart>
        </ResponsiveContainer>
      </div>

      {/* Your best video */}
      <div className="rounded-xl bg-surface border border-border p-4">
        <div className="flex items-center gap-2 mb-2">
          <Trophy size={14} className="text-yellow-400" />
          <h2 className="text-sm font-medium text-gray-400">Your best-performing video</h2>
        </div>
        <p className="text-gray-200">{data.best_video_title}</p>
      </div>

      {/* Tag gaps preview */}
      {data.tag_gaps.length > 0 && (
        <div className="rounded-xl bg-surface border border-border p-4">
          <h2 className="text-sm font-medium text-gray-400 mb-3">
            Tags competitors use that you don&apos;t — opportunity to add
          </h2>
          <div className="flex flex-wrap gap-1.5">
            {data.tag_gaps.slice(0, 12).map((g: TagGapInsight) => (
              <span key={g.tag} className="px-2 py-1 rounded-full bg-accent/10 border border-accent/25 text-xs text-accent" title={`${g.competitor_usage} competitor videos`}>
                {g.tag}
                <span className="ml-1 text-[10px] opacity-70">×{g.competitor_usage}</span>
              </span>
            ))}
          </div>
        </div>
      )}

      {/* Knowledge Graph visualization */}
      <div className="rounded-xl bg-surface border border-border p-4">
        <div className="flex items-center gap-2 mb-3">
          <Network size={14} className="text-accent" />
          <h2 className="text-sm font-medium text-gray-400">Knowledge Graph</h2>
        </div>
        {graphSvg ? (
          <div
            className="w-full overflow-hidden rounded-lg bg-gray-900/50"
            dangerouslySetInnerHTML={{ __html: graphSvg }}
          />
        ) : (
          <div className="text-center text-gray-500 py-8">
            {graphError ? (
              <>
                <p className="text-sm">{graphError}</p>
                <p className="text-xs mt-1">Run <code className="text-accent">tubeforge kg build</code> to generate the graph.</p>
              </>
            ) : (
              <p className="text-sm">Graph visualization loading...</p>
            )}
          </div>
        )}
        <p className="text-xs text-gray-600 mt-2">
          Node size = centrality (PageRank). Color = entity type.
        </p>
      </div>
    </div>
  );
}
