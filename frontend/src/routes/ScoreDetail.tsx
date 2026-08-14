import { useState, useCallback, useEffect } from 'react';
import { useParams } from 'react-router-dom';
import { useRpc } from '../lib/rpc';
import type { ScoreDetail as ScoreDetailType, GraphScores } from '../lib/types';
import { Loader2, Network } from 'lucide-react';

export default function ScoreDetail() {
  const { id = '' } = useParams();
  const { call, connected } = useRpc();
  const [data, setData] = useState<ScoreDetailType | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchDetail = useCallback(async () => {
    if (!connected || !id) return;
    setLoading(true);
    setError(null);
    try {
      const result = (await call('scores.detail', { id })) as ScoreDetailType;
      setData(result);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }, [call, connected, id]);

  useEffect(() => {
    fetchDetail();
  }, [fetchDetail]);

  if (loading) {
    return (
      <div className="p-12 flex justify-center text-gray-500">
        <Loader2 size={20} className="animate-spin mr-2" /> Loading score detail...
      </div>
    );
  }

  if (error || !data) {
    return (
      <div className="p-12 text-center text-gray-500">
        {error || 'Failed to load score detail'}
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold">{data.title}</h1>

      <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
        <div className="rounded-xl bg-surface border border-border p-4">
          <div className="text-xs uppercase tracking-wide text-gray-500">SEO Total</div>
          <div className="text-3xl font-bold text-accent">{data.seo_total.toFixed(1)}</div>
        </div>
        <div className="rounded-xl bg-surface border border-border p-4">
          <div className="text-xs uppercase tracking-wide text-gray-500">GEO Total</div>
          <div className="text-3xl font-bold text-accent">{data.geo_total.toFixed(1)}</div>
        </div>
        <div className="rounded-xl bg-surface border border-border p-4">
          <div className="text-xs uppercase tracking-wide text-gray-500">Overall</div>
          <div className="text-3xl font-bold text-accent">{data.total.toFixed(1)}</div>
        </div>
      </div>

      {/* SEO Components */}
      <section className="rounded-xl bg-surface border border-border p-4">
        <h2 className="text-sm font-medium text-gray-400 mb-3">SEO Components</h2>
        <div className="space-y-2">
          {Object.entries(data.seo_components).map(([key, value]) => (
            <div key={key} className="flex items-center justify-between">
              <span className="text-sm text-gray-400">{key.replace(/_/g, ' ')}</span>
              <div className="flex items-center gap-2">
                <div className="w-24 h-1.5 bg-gray-800 rounded-full overflow-hidden">
                  <div className="h-full bg-blue-500 rounded-full" style={{ width: `${Math.min(value, 100)}%` }} />
                </div>
                <span className="text-sm font-mono w-10 text-right">{value.toFixed(0)}</span>
              </div>
            </div>
          ))}
        </div>
      </section>

      {/* GEO Components */}
      <section className="rounded-xl bg-surface border border-border p-4">
        <h2 className="text-sm font-medium text-gray-400 mb-3">GEO Components</h2>
        <div className="space-y-2">
          {Object.entries(data.geo_components).map(([key, value]) => (
            <div key={key} className="flex items-center justify-between">
              <span className="text-sm text-gray-400">{key.replace(/_/g, ' ')}</span>
              <div className="flex items-center gap-2">
                <div className="w-24 h-1.5 bg-gray-800 rounded-full overflow-hidden">
                  <div className="h-full bg-purple-500 rounded-full" style={{ width: `${Math.min(value, 100)}%` }} />
                </div>
                <span className="text-sm font-mono w-10 text-right">{value.toFixed(0)}</span>
              </div>
            </div>
          ))}
        </div>
      </section>

      {/* Graph Scores (from Knowledge Graph — when available) */}
      {'graph_scores' in data && (data as unknown as { graph_scores: GraphScores | null }).graph_scores && (
        <section className="rounded-xl bg-surface border border-border p-4">
          <div className="flex items-center gap-2 mb-3">
            <Network size={14} className="text-accent" />
            <h2 className="text-sm font-medium text-gray-400">Knowledge Graph Signals</h2>
          </div>
          <div className="space-y-2">
            {(() => {
              const gs = (data as unknown as { graph_scores: GraphScores }).graph_scores;
              return (
                <>
                  <div className="flex items-center justify-between">
                    <span className="text-sm text-gray-400">Tag Authority</span>
                    <div className="flex items-center gap-2">
                      <div className="w-24 h-1.5 bg-gray-800 rounded-full overflow-hidden">
                        <div className="h-full bg-green-500 rounded-full" style={{ width: `${Math.min(gs.tag_authority, 100)}%` }} />
                      </div>
                      <span className="text-sm font-mono w-10 text-right">{gs.tag_authority.toFixed(0)}</span>
                    </div>
                  </div>
                  <div className="flex items-center justify-between">
                    <span className="text-sm text-gray-400">Topic Dominance</span>
                    <div className="flex items-center gap-2">
                      <div className="w-24 h-1.5 bg-gray-800 rounded-full overflow-hidden">
                        <div className="h-full bg-green-500 rounded-full" style={{ width: `${Math.min(gs.topic_dominance, 100)}%` }} />
                      </div>
                      <span className="text-sm font-mono w-10 text-right">{gs.topic_dominance.toFixed(0)}</span>
                    </div>
                  </div>
                  <div className="flex items-center justify-between">
                    <span className="text-sm text-gray-400">Keyword Competition</span>
                    <div className="flex items-center gap-2">
                      <div className="w-24 h-1.5 bg-gray-800 rounded-full overflow-hidden">
                        <div className="h-full bg-orange-500 rounded-full" style={{ width: `${Math.min(gs.keyword_competition, 100)}%` }} />
                      </div>
                      <span className="text-sm font-mono w-10 text-right">{gs.keyword_competition.toFixed(0)}</span>
                    </div>
                  </div>
                </>
              );
            })()}
          </div>
          <p className="text-xs text-gray-600 mt-2">Run <code>tubeforge kg build</code> to compute graph signals.</p>
        </section>
      )}
    </div>
  );
}
