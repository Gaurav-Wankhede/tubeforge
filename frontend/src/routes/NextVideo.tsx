import { useCallback, useEffect, useState } from 'react';
import { useRpc } from '../lib/rpc';
import type { AnalysisNextVideo, NextVideoRecommendation } from '../lib/types';
import { Target, Loader2, Trophy } from 'lucide-react';
import { FreshnessBadge } from '../components/FreshnessBadge';

function RecommendationCard({ rec, rank }: { rec: NextVideoRecommendation; rank: number }) {
  return (
    <div className={`rounded-xl bg-surface border p-5 ${rank === 1 ? 'border-accent/50' : 'border-border'}`}>
      <div className="flex items-center gap-2 mb-2">
        <span className={`text-xs font-bold px-2 py-0.5 rounded-full ${rank === 1 ? 'bg-accent/20 text-accent' : 'bg-gray-700/40 text-gray-400'}`}>
          #{rank}
        </span>
        {rank === 1 && (
          <span className="inline-flex items-center gap-1 text-xs text-yellow-400">
            <Trophy size={12} /> Top pick
          </span>
        )}
        <span
          className={`text-xs font-bold px-2 py-0.5 rounded-full ${
            rec.prediction === 'Very High' ? 'bg-green-500/20 text-green-400'
            : rec.prediction === 'High' ? 'bg-blue-500/20 text-blue-400'
            : rec.prediction === 'Medium' ? 'bg-yellow-500/20 text-yellow-400'
            : 'bg-gray-600/30 text-gray-400'
          }`}
        >
          View prediction: {rec.prediction}
        </span>
        <span className="text-xs text-gray-500">{rec.verdict} · reliability {rec.reliability}</span>
      </div>

      <h3 className="text-lg font-bold mb-1">{rec.title}</h3>
      <div className="text-xs text-gray-400 mb-2">Topic: <span className="text-gray-200">{rec.topic}</span></div>

      {/* The actionable "why make THIS" */}
      <div className="rounded-lg bg-accent/5 border border-accent/20 px-3 py-2 mb-3 text-xs text-gray-200">
        <span className="font-semibold text-accent">Make this because: </span>
        {rec.why}
      </div>

      <div className="grid grid-cols-3 gap-3 mb-3">
        <div>
          <div className="text-[11px] text-gray-500">Opportunity</div>
          <div className="font-bold text-green-400">{rec.opportunity_score.toFixed(0)}</div>
        </div>
        <div>
          <div className="text-[11px] text-gray-500">Competition</div>
          <div className="font-bold text-red-400">{rec.competition_score.toFixed(0)}</div>
        </div>
        <div>
          <div className="text-[11px] text-gray-500">Volume</div>
          <div className="font-bold">{rec.volume_label}</div>
        </div>
      </div>

      <div className="mb-2">
        <div className="text-[11px] text-gray-500 mb-1">Description</div>
        <p className="text-sm text-gray-300">{rec.description}</p>
      </div>

      <div>
        <div className="text-[11px] text-gray-500 mb-1">Tags</div>
        <div className="flex flex-wrap gap-1.5">
          {rec.tags.map((t) => (
            <span key={t} className="px-2 py-1 rounded-full bg-accent/10 border border-accent/25 text-xs text-accent">
              {t}
            </span>
          ))}
        </div>
      </div>
    </div>
  );
}

export default function NextVideo() {
  const { call, connected } = useRpc();
  const [data, setData] = useState<AnalysisNextVideo | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchRecommendations = useCallback(async () => {
    if (!connected) return;
    setLoading(true);
    setError(null);
    try {
      const result = (await call('analysis.next-video', { limit: 5 })) as AnalysisNextVideo;
      setData(result);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }, [call, connected]);

  useEffect(() => {
    fetchRecommendations();
  }, [fetchRecommendations]);

  const recs = data?.recommendations ?? [];

  if (loading) {
    return (
      <div className="p-12 flex justify-center text-gray-500">
        <Loader2 size={20} className="animate-spin mr-2" /> Computing recommendations...
      </div>
    );
  }

  if (error || recs.length === 0) {
    return (
      <div className="p-12 text-center text-gray-500">
        <Target size={32} className="mx-auto mb-3 opacity-50" />
        {error || 'No recommendations available — research keywords or add more videos to your corpus.'}
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h1 className="text-2xl font-bold">Next Video</h1>
          <FreshnessBadge at={data?.research_at ?? null} />
        </div>
        <button
          onClick={fetchRecommendations}
          disabled={loading || !connected}
          className="text-xs text-accent hover:underline disabled:opacity-50"
        >
          {loading ? 'Computing...' : 'Refresh'}
        </button>
      </div>

      <div className="text-xs text-gray-500">
        {recs.length} ranked topics — the top pick is your best next move; the rest are alternates.
      </div>

      <div className="space-y-4">
        {recs.map((rec, i) => (
          <RecommendationCard key={rec.topic} rec={rec} rank={i + 1} />
        ))}
      </div>
    </div>
  );
}
