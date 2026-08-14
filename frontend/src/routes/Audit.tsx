import { useState, useCallback, useEffect } from 'react';
import { Link } from 'react-router-dom';
import { useRpc } from '../lib/rpc';
import type { ChannelAudit as AuditType } from '../lib/types';
import { ClipboardCheck, Loader2, ArrowUpRight } from 'lucide-react';

type ActionItem = {
  area: string
  what: string
  why: string
  impact: 'High' | 'Medium' | 'Low'
  score: number
}

const impactColor: Record<string, string> = {
  High: 'text-red-400',
  Medium: 'text-yellow-400',
  Low: 'text-gray-400',
}

function gradeColor(g: string) {
  if (g === 'A') return 'text-green-400';
  if (g === 'B') return 'text-yellow-400';
  if (g === 'C') return 'text-orange-400';
  return 'text-red-400';
}

function scoreColor(s: number) {
  if (s >= 80) return 'bg-green-500/15 text-green-400';
  if (s >= 60) return 'bg-yellow-500/15 text-yellow-400';
  if (s >= 40) return 'bg-orange-500/15 text-orange-400';
  return 'bg-red-500/15 text-red-400';
}

const componentLabels: Record<string, string> = {
  metadata: 'Metadata quality',
  consistency: 'Upload consistency',
  engagement: 'Engagement',
  tags: 'Tag usage',
  series: 'Series strength',
  authority: 'Authority',
};

export default function Audit() {
  const { call, connected } = useRpc();
  const [data, setData] = useState<AuditType[]>([]);
  const [actions, setActions] = useState<ActionItem[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<AuditType | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchAudit = useCallback(async () => {
    if (!connected) return;
    setLoading(true);
    setError(null);
    try {
      const result = (await call('audit.get')) as { audit: AuditType; actions?: ActionItem[] };
      setData([result.audit]);
      setActions(result.actions ?? []);
    } catch (e) {
      setError((e as Error).message || 'Failed to load audit');
    } finally {
      setLoading(false);
    }
  }, [call, connected]);

  useEffect(() => {
    fetchAudit();
  }, [fetchAudit]);

  const fetchDetail = useCallback(async (_id: string) => {
    try {
      const result = (await call('audit.get')) as { audit: AuditType };
      setDetail(result.audit);
    } catch (e) {
      setError((e as Error).message || 'Failed to load audit detail');
    }
  }, [call]);

  const handleSelect = (id: string) => {
    const next = selectedId === id ? null : id;
    setSelectedId(next);
    if (next) fetchDetail(next);
  };

  if (loading) {
    return (
      <div className="p-12 flex justify-center text-gray-500">
        <Loader2 size={20} className="animate-spin mr-2" /> Auditing channels...
      </div>
    );
  }

  if (error) {
    return (
      <div className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-400">
        {error}
      </div>
    );
  }

  if (data.length === 0) {
    return (
      <div className="p-12 text-center text-gray-500">
        <ClipboardCheck size={32} className="mx-auto mb-3 opacity-50" />
        No channels to audit yet — run <code className="text-gray-400">tubeforge ingest</code>.
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <ClipboardCheck size={20} className="text-accent" />
          <h1 className="text-2xl font-bold">Channel Audit</h1>
        </div>
        <button
          onClick={fetchAudit}
          disabled={loading || !connected}
          className="text-xs text-accent hover:underline disabled:opacity-50"
        >
          {loading ? 'Auditing...' : 'Refresh'}
        </button>
      </div>
      <p className="text-sm text-gray-500 -mt-3">
        VidIQ-style channel health score — metadata, consistency, engagement, tags, series and
        authority, computed from your stored corpus.
      </p>

      {/* Actionable: "fix these in order" */}
      {actions.length > 0 && (
        <div className="rounded-xl bg-surface border border-accent/30 p-4">
          <div className="flex items-center gap-2 mb-3">
            <ArrowUpRight size={14} className="text-accent" />
            <h2 className="text-sm font-semibold text-accent">Do these, in order, to grow your channel</h2>
          </div>
          <div className="space-y-2">
            {actions.map((a, i) => (
              <div key={a.area} className="flex items-start gap-3 rounded-lg bg-gray-800/30 border border-border p-3">
                <span className="text-xs font-bold text-gray-500 w-5 shrink-0">{i + 1}</span>
                <div className="flex-1 min-w-0">
                  <div className="text-sm text-gray-100">{a.what}</div>
                  <div className="text-xs text-gray-500 mt-0.5">{a.why}</div>
                </div>
                <span className={`text-xs font-bold shrink-0 ${impactColor[a.impact]}`}>{a.impact} impact</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Channel score cards */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {data.map((a: AuditType) => (
          <div
            key={a.channel_id}
            className={`rounded-xl bg-surface border p-5 cursor-pointer transition-colors ${
              selectedId === a.channel_id ? 'border-accent/60' : 'border-border hover:border-accent/40'
            }`}
            onClick={() => handleSelect(a.channel_id)}
          >
            <div className="flex items-start justify-between">
              <div>
                <Link to="/scorecard" className="font-semibold hover:text-accent">
                  {a.channel_name}
                </Link>
                <div className="text-xs text-gray-500 font-mono">{a.channel_id.slice(0, 12)}…</div>
              </div>
              <div className="text-right">
                <div className={`text-3xl font-black ${gradeColor(a.grade)}`}>
                  {a.total_score.toFixed(0)}
                </div>
                <div className="text-xs text-gray-500">grade {a.grade}</div>
              </div>
            </div>

            {/* Component bars */}
            <div className="mt-4 space-y-2">
              {a.components.map((c) => (
                <div key={c.name} className="space-y-0.5">
                  <div className="flex justify-between text-xs">
                    <span className="text-gray-400">{componentLabels[c.name] ?? c.name}</span>
                    <span className={`font-mono ${c.score >= 70 ? 'text-green-400' : c.score >= 40 ? 'text-yellow-400' : 'text-red-400'}`}>
                      {c.score.toFixed(0)}
                    </span>
                  </div>
                  <div className="h-1 bg-gray-800 rounded-full overflow-hidden">
                    <div
                      className={`h-full rounded-full ${c.score >= 70 ? 'bg-green-500' : c.score >= 40 ? 'bg-yellow-500' : 'bg-red-500'}`}
                      style={{ width: `${Math.min(c.score, 100)}%` }}
                    />
                  </div>
                </div>
              ))}
            </div>

            {/* Verdict */}
            <div className="mt-3 text-xs text-gray-500 leading-relaxed">{a.verdict}</div>
          </div>
        ))}
      </div>

      {/* Weakest-lever summary */}
      <div className="rounded-xl bg-surface border border-border p-4 text-sm">
        {data.map((a: AuditType) => {
          const weakest = [...a.components].sort((x, y) => x.score - y.score)[0];
          if (!weakest) return null;
          return (
            <div key={a.channel_id} className="flex items-center gap-3 py-1">
              <span className="font-medium w-40 truncate">{a.channel_name}</span>
              <span className={`px-2 py-0.5 rounded text-xs font-bold ${scoreColor(weakest.score)}`}>
                {weakest.score.toFixed(0)}
              </span>
              <span className="text-gray-400 text-xs">
                biggest lever: {componentLabels[weakest.name] ?? weakest.name}
              </span>
            </div>
          );
        })}
      </div>

      {/* Channel detail drilldown */}
      {selectedId && detail && (
        <div className="rounded-xl bg-surface border border-accent/40 p-5">
          <div className="flex items-center justify-between mb-4">
            <div>
              <h2 className="text-lg font-bold">{detail.channel_name}</h2>
              <div className="text-xs text-gray-500 font-mono">{detail.channel_id}</div>
            </div>
            <div className="text-right">
              <div className={`text-2xl font-black ${gradeColor(detail.grade)}`}>
                {detail.total_score.toFixed(0)}
              </div>
              <div className="text-xs text-gray-500">grade {detail.grade}</div>
            </div>
          </div>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-x-6 gap-y-3">
            {detail.components.map((c) => (
              <div key={c.name} className="space-y-0.5">
                <div className="flex justify-between text-xs">
                  <span className="text-gray-400">{componentLabels[c.name] ?? c.name}</span>
                  <span className={`font-mono ${c.score >= 70 ? 'text-green-400' : c.score >= 40 ? 'text-yellow-400' : 'text-red-400'}`}>
                    {c.score.toFixed(0)}
                  </span>
                </div>
                <div className="h-1 bg-gray-800 rounded-full overflow-hidden">
                  <div
                    className={`h-full rounded-full ${c.score >= 70 ? 'bg-green-500' : c.score >= 40 ? 'bg-yellow-500' : 'bg-red-500'}`}
                    style={{ width: `${Math.min(c.score, 100)}%` }}
                  />
                </div>
                {c.detail && <div className="text-[10px] text-gray-600">{c.detail}</div>}
              </div>
            ))}
          </div>
          <p className="mt-3 text-xs text-gray-400 leading-relaxed">{detail.verdict}</p>
        </div>
      )}
    </div>
  );
}
