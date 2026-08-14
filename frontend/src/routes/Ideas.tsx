import { useCallback, useState } from 'react';
import { Lightbulb, RefreshCw, Database, Clock, Network } from 'lucide-react';
import { useRpc } from '../lib/rpc';
import type { Idea, IdeaRationale, GraphIdea } from '../lib/types';

function verdictFromScore(score: number): { label: string; color: string; description: string } {
  if (score >= 70) return {
    label: 'High Potential',
    color: 'text-green-400 bg-green-500/10 border-green-500/30',
    description: 'Strong SEO fit + high competitor gap. Prioritize this topic.',
  };
  if (score >= 50) return {
    label: 'Promising',
    color: 'text-blue-400 bg-blue-500/10 border-blue-500/30',
    description: 'Solid opportunity with room to differentiate.',
  };
  if (score >= 30) return {
    label: 'Moderate',
    color: 'text-yellow-400 bg-yellow-500/10 border-yellow-500/30',
    description: 'Worth exploring if it aligns with your content strategy.',
  };
  return {
    label: 'Low Signal',
    color: 'text-gray-400 bg-gray-500/10 border-gray-500/30',
    description: 'Weak signals — consider only if niche-relevant.',
  };
}

function ScoreBar({ label, value, max, color, icon: Icon }: { label: string; value: number; max: number; color: string; icon: typeof Lightbulb }) {
  const pct = Math.min((value / max) * 100, 100);
  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between text-xs">
        <span className="flex items-center gap-1.5 text-gray-400">
          <Icon size={12} />
          {label}
        </span>
        <span className="font-mono text-gray-300">{value.toFixed(1)}</span>
      </div>
      <div className="h-1.5 rounded-full bg-gray-700/50 overflow-hidden">
        <div className={`h-full rounded-full ${color}`} style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}

function AnalysisCard({ rationale, score }: { rationale: IdeaRationale; score: number }) {
  const verdict = verdictFromScore(score);
  const seoPct = Math.min(rationale.seo_total, 100);
  const fitPct = Math.min(rationale.idea_fit, 100);
  const gapPct = Math.min(rationale.competitor_gap, 100);

  return (
    <div className="mt-3 space-y-3">
      <div className={`inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full border text-xs font-medium ${verdict.color}`}>
        {verdict.label}
      </div>
      <p className="text-xs text-gray-500 -mt-1">{verdict.description}</p>

      <div className="space-y-2 pt-1">
        <ScoreBar label="SEO Score" value={seoPct} max={100} color="bg-blue-500" icon={Lightbulb} />
        <ScoreBar label="Idea Fit" value={fitPct} max={100} color="bg-purple-500" icon={Lightbulb} />
        <ScoreBar label="Competitor Gap" value={gapPct} max={100} color="bg-orange-500" icon={Lightbulb} />
      </div>

      <div className="grid grid-cols-2 gap-2 pt-1">
        <div className="rounded-lg bg-gray-800/40 border border-gray-700/40 px-2.5 py-2">
          <div className="text-[10px] uppercase tracking-wider text-gray-500 mb-0.5">Keyword</div>
          <div className="text-xs text-gray-200 font-medium truncate">{rationale.keyword || '—'}</div>
        </div>
        <div className="rounded-lg bg-gray-800/40 border border-gray-700/40 px-2.5 py-2">
          <div className="text-[10px] uppercase tracking-wider text-gray-500 mb-0.5">Demand Matches</div>
          <div className="text-xs text-gray-200 font-medium">{rationale.demand_matches} competitor videos</div>
        </div>
        <div className="rounded-lg bg-gray-800/40 border border-gray-700/40 px-2.5 py-2">
          <div className="text-[10px] uppercase tracking-wider text-gray-500 mb-0.5">Channel Centrality</div>
          <div className="text-xs text-gray-200 font-medium">{(rationale.centrality * 100).toFixed(0)}%</div>
        </div>
        <div className="rounded-lg bg-gray-800/40 border border-gray-700/40 px-2.5 py-2">
          <div className="text-[10px] uppercase tracking-wider text-gray-500 mb-0.5">Engagement Boost</div>
          <div className="text-xs text-gray-200 font-medium">+{(rationale.engagement_boost * 100).toFixed(1)}%</div>
        </div>
      </div>
    </div>
  );
}

function parseRationale(r: Idea['rationale']): IdeaRationale {
  if (typeof r === 'string') {
    try { return JSON.parse(r) } catch { return emptyRationale }
  }
  return r || emptyRationale;
}

export default function Ideas() {
  const { call, connected } = useRpc();
  const [ideas, setIdeas] = useState<Idea[]>([]);
  const [graphIdeas, setGraphIdeas] = useState<GraphIdea[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [meta, setMeta] = useState<{ corpus_size: number; generated_at: string } | null>(null);
  const [progress, setProgress] = useState<string | null>(null);

  const analyze = useCallback(async () => {
    setLoading(true);
    setError(null);
    setProgress('Connecting...');
    try {
      const result = (await call('ideas.analyze', { limit: 25 }, (_p, msg) => {
        setProgress(msg);
      })) as { ideas: Idea[]; corpus_size: number; generated_at: string; graph_ideas?: GraphIdea[] | null; note?: string };
      setIdeas(result.ideas);
      setGraphIdeas(result.graph_ideas ?? null);
      setMeta({ corpus_size: result.corpus_size, generated_at: result.generated_at });
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
      setProgress(null);
    }
  }, [call]);

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Lightbulb size={20} className="text-accent" />
          <h1 className="text-2xl font-bold">Content Ideas</h1>
          <span className={`w-2 h-2 rounded-full ${connected ? 'bg-green-400' : 'bg-red-400'}`} title={connected ? 'Live' : 'Disconnected'} />
        </div>
        <button
          onClick={analyze}
          disabled={loading || !connected}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-accent/10 border border-accent/25 text-xs text-accent hover:bg-accent/20 transition-colors disabled:opacity-50"
        >
          <RefreshCw size={12} className={loading ? 'animate-spin' : ''} />
          {loading ? 'Analyzing...' : 'Analyze'}
        </button>
      </div>

      {meta && (
        <div className="flex items-center gap-4 text-xs text-gray-500">
          <span className="flex items-center gap-1">
            <Database size={11} />
            {meta.corpus_size} videos
          </span>
          <span className="flex items-center gap-1">
            <Clock size={11} />
            {new Date(meta.generated_at).toLocaleTimeString()}
          </span>
          <span>{ideas.length} recommendations</span>
        </div>
      )}

      {loading && progress && (
        <div className="flex items-center gap-2 text-xs text-accent bg-accent/5 border border-accent/20 rounded-lg px-3 py-2">
          <RefreshCw size={12} className="animate-spin" />
          {progress}
        </div>
      )}

      {error && (
        <div className="rounded-xl bg-surface border border-border p-8 text-center text-red-400">
          Analysis failed: {error}
        </div>
      )}

      {!loading && !error && ideas.length === 0 && (
        <div className="rounded-xl bg-surface border border-border p-12 text-center text-gray-500">
          <Lightbulb size={32} className="mx-auto mb-3 opacity-50" />
          Click &quot;Analyze&quot; to compute fresh content ideas from your corpus.
        </div>
      )}

      {/* Graph-based ideas (from Knowledge Graph — when available) */}
      {graphIdeas && graphIdeas.length > 0 && (
        <div className="rounded-xl bg-surface border border-accent/30 p-4">
          <div className="flex items-center gap-2 mb-3">
            <Network size={14} className="text-accent" />
            <h2 className="text-sm font-semibold text-accent">Knowledge Graph Suggestions</h2>
          </div>
          <div className="space-y-2">
            {graphIdeas.map((g, i) => (
              <div key={i} className="flex items-center justify-between rounded-lg bg-gray-800/30 border border-border p-3">
                <div className="flex-1 min-w-0">
                  <div className="text-sm font-medium text-gray-100">{g.title}</div>
                  <div className="text-xs text-gray-500 mt-0.5">{g.rationale}</div>
                </div>
                <span className="ml-3 text-xs font-mono text-accent">{g.score.toFixed(0)}</span>
              </div>
            ))}
          </div>
          <p className="text-xs text-gray-600 mt-2">Detected via community gap analysis. Run <code>tubeforge kg build</code> to update.</p>
        </div>
      )}

      <div className="space-y-3">
        {ideas.map((idea: Idea) => {
          const rationale = parseRationale(idea.rationale);
          return (
            <div
              key={idea.id}
              className="rounded-xl bg-surface border border-border p-4 hover:border-accent/30 transition-colors"
            >
              <div className="flex items-start justify-between">
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2 mb-1">
                    <span className="text-xs font-mono text-gray-500">#{idea.id}</span>
                    <span className="text-xs font-mono text-gray-400">
                      Score: <span className="text-gray-200 font-medium">{idea.score.toFixed(1)}</span>
                    </span>
                  </div>
                  <h3 className="font-medium text-gray-100">{idea.title}</h3>
                </div>
              </div>
              <AnalysisCard rationale={rationale} score={idea.score} />
            </div>
          );
        })}
      </div>
    </div>
  );
}

const emptyRationale: IdeaRationale = {
  seo_total: 0,
  idea_fit: 0,
  competitor_gap: 0,
  engagement_boost: 0,
  centrality: 0,
  demand_matches: 0,
  keyword: '',
  source_channel: '',
};
