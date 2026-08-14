import { useState, useCallback } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import {
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
  Legend,
} from 'recharts';
import {
  TrendingUp,
  TrendingDown,
  Minus,
  Search,
  Loader2,
  Target,
  Users,
  Sparkles,
  Database,
  Hash,
  Tag as TagIcon,
  History,
} from 'lucide-react';
import { api } from '../lib/api';
import { useRpc } from '../lib/rpc';
import type { Keyword, KeywordResearch, ResearchSnapshot } from '../lib/types';

const trendIcon = (t: string) => {
  if (t === 'rising') return <TrendingUp size={14} className="text-green-400" />;
  if (t === 'declining') return <TrendingDown size={14} className="text-red-400" />;
  return <Minus size={14} className="text-gray-400" />;
};

function Gauge({ label, value, color }: { label: string; value: number; color: string }) {
  return (
    <div className="rounded-xl border border-border bg-surface p-4">
      <div className="text-xs uppercase tracking-wide text-gray-500">{label}</div>
      <div className={`mt-1 text-2xl font-black ${color}`}>{value.toFixed(0)}</div>
      <div className="mt-2 h-1.5 bg-gray-800 rounded-full overflow-hidden">
        <div
          className={`h-full rounded-full ${color.replace('text-', 'bg-')}`}
          style={{ width: `${Math.min(value, 100)}%` }}
        />
      </div>
    </div>
  );
}

function formatViews(v: number | null) {
  if (v === null || v === undefined) return '—';
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
  if (v >= 1_000) return `${(v / 1_000).toFixed(1)}k`;
  return String(v);
}

export default function Keywords() {
  const [query, setQuery] = useState('');
  const [submitted, setSubmitted] = useState('');
  const { call, connected } = useRpc();

  // Runtime data via WebSocket RPC
  const [keywords, setKeywords] = useState<Keyword[]>([]);
  const [trending, setTrending] = useState<Keyword[]>([]);
  const [rpcLoading, setRpcLoading] = useState(false);

  const fetchKeywords = useCallback(async () => {
    if (!connected) return;
    setRpcLoading(true);
    try {
      const result = (await call('keywords.list')) as { keywords: Keyword[] };
      setKeywords(result.keywords);
    } catch (e) {
      console.warn('keywords.list failed:', (e as Error).message);
    } finally {
      setRpcLoading(false);
    }
  }, [call, connected]);

  const fetchTrending = useCallback(async () => {
    if (!connected) return;
    try {
      const result = (await call('keywords.trending')) as { trending: Keyword[] };
      setTrending(result.trending);
    } catch (e) {
      console.warn('keywords.trending failed:', (e as Error).message);
    }
  }, [call, connected]);

  // Fetch on connect
  useState(() => {
    fetchKeywords();
    fetchTrending();
  });

  // Keyword research (live YouTube search via HTTP — requires yt-dlp)
  const { data: research, isLoading: researching, error, isError } = useQuery({
    queryKey: ['inspect', submitted],
    queryFn: () => api.inspectKeyword(submitted),
    enabled: submitted.length > 0,
    retry: false,
    staleTime: 5 * 60_000,
  });

  const { data: history } = useQuery({
    queryKey: ['keywordHistory', submitted],
    queryFn: () => api.keywordHistory(submitted),
    enabled: submitted.length > 0,
    staleTime: 30_000,
  });

  const submit = () => {
    const q = query.trim();
    if (q) setSubmitted(q);
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold">Keywords</h1>
        <button
          onClick={() => { fetchKeywords(); fetchTrending(); }}
          disabled={rpcLoading || !connected}
          className="text-xs text-accent hover:underline disabled:opacity-50"
        >
          {rpcLoading ? 'Loading...' : 'Refresh'}
        </button>
      </div>

      {/* VidIQ-style research: input box → analysis */}
      <section className="rounded-xl border border-border bg-surface p-5">
        <div className="flex items-center gap-2 mb-1">
          <Sparkles size={16} className="text-accent" />
          <h2 className="text-sm font-semibold uppercase tracking-wide text-gray-400">
            Keyword research
          </h2>
        </div>
        <p className="text-xs text-gray-500 mb-3">
          Type a topic — TubeForge searches YouTube right now (real ranking videos) and your
          stored database, then scores the opportunity.
        </p>
        <div className="flex gap-2">
          <div className="relative flex-1">
            <Search size={16} className="absolute left-3 top-1/2 -translate-y-1/2 text-gray-500" />
            <input
              type="text"
              placeholder="e.g. rust async tokio"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && submit()}
              className="w-full pl-9 pr-3 py-2.5 bg-surface border border-border rounded-lg text-sm focus:outline-none focus:border-accent"
            />
          </div>
          <button
            onClick={submit}
            disabled={researching || !query.trim()}
            className="px-4 py-2.5 rounded-lg bg-accent/15 border border-accent/30 text-sm font-semibold text-accent hover:bg-accent/25 disabled:opacity-50 transition-colors"
          >
            {researching ? <Loader2 size={14} className="animate-spin inline" /> : 'Research'}
          </button>
        </div>
      </section>

      {/* Research results */}
      {researching && (
        <div className="flex justify-center py-8 text-gray-500">
          <Loader2 size={20} className="animate-spin mr-2" /> Searching YouTube for "
          {submitted}"...
        </div>
      )}

      {isError && !researching && (
        <div className="rounded-xl border border-red-500/30 bg-red-500/10 p-4 text-sm text-red-400">
          Research failed — {String(error)}. Is yt-dlp enabled? (set{' '}
          <code>TUBEFORGE_YTDLP_ENABLED=true</code>)
        </div>
      )}

      {research && !researching && (
        <>
          <ResearchResults r={research} />
          <ResearchHistory h={history?.snapshots ?? []} keyword={submitted} />
        </>
      )}

      {/* Trending keywords */}
      <section>
        <h2 className="text-sm font-semibold uppercase tracking-wide text-gray-400 mb-3">
          Trending topics (latest research, by opportunity)
        </h2>
        {trending.length === 0 ? (
          <div className="rounded-xl bg-surface border border-border p-8 text-center text-gray-500 text-sm">
            No keyword research yet. Run{' '}
            <code className="text-accent">tubeforge keywords research &quot;topic&quot;</code> or
            use the research box above to find trending topics.
          </div>
        ) : (
          <div className="rounded-xl bg-surface border border-border overflow-hidden">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border">
                  <th className="px-4 py-3 text-left font-medium text-gray-400">Keyword</th>
                  <th className="px-4 py-3 text-right font-medium text-gray-400">Rank</th>
                </tr>
              </thead>
              <tbody>
                {trending.map((t) => (
                  <tr
                    key={t.keyword}
                    className="border-b border-border/50 hover:bg-surface-hover cursor-pointer"
                    onClick={() => { setQuery(t.keyword); setSubmitted(t.keyword); }}
                  >
                    <td className="px-4 py-3 font-medium">{t.keyword}</td>
                    <td className="px-4 py-3 text-gray-400">#{t.rank}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>

      {/* Tracked keywords table */}
      <section>
        <h2 className="text-sm font-semibold uppercase tracking-wide text-gray-400 mb-3">
          Tracked keywords
        </h2>
        {rpcLoading ? (
          <div className="text-gray-500">Loading keywords...</div>
        ) : keywords.length === 0 ? (
          <div className="rounded-xl bg-surface border border-border p-8 text-center text-gray-500 text-sm">
            No tracked keywords yet. Run{' '}
            <code className="text-accent">tubeforge keywords add &lt;kw&gt;</code> to track
            rankings over time.
          </div>
        ) : (
          <div className="rounded-xl bg-surface border border-border overflow-hidden">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border">
                  <th className="px-4 py-3 text-left font-medium text-gray-400">Keyword</th>
                  <th className="px-4 py-3 text-left font-medium text-gray-400">Rank</th>
                  <th className="px-4 py-3 text-left font-medium text-gray-400">Trend</th>
                  <th className="px-4 py-3 text-left font-medium text-gray-400">Sparkline</th>
                </tr>
              </thead>
              <tbody>
                {keywords.map((kw: Keyword) => (
                  <tr
                    key={kw.keyword}
                    className="border-b border-border/50 hover:bg-surface-hover transition-colors"
                  >
                    <td className="px-4 py-3 font-medium">{kw.keyword}</td>
                    <td className="px-4 py-3 text-gray-400">#{kw.rank}</td>
                    <td className="px-4 py-3 flex items-center gap-1.5">
                      {trendIcon(kw.trend)}
                      <span className="capitalize text-xs">{kw.trend}</span>
                    </td>
                    <td className="px-4 py-3">
                      <div className="flex items-end gap-px h-5">
                        {kw.sparkline.map((v, i) => (
                          <div
                            key={i}
                            className="w-1 rounded-t bg-accent/60"
                            style={{
                              height: `${Math.max(2, (v / Math.max(...kw.sparkline)) * 20)}px`,
                            }}
                          />
                        ))}
                      </div>
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

function ResearchHistory({ h, keyword }: { h: ResearchSnapshot[]; keyword: string }) {
  if (h.length < 2) return null;
  const data = h.map((s) => ({
    at: new Date(s.at).toLocaleDateString(),
    opportunity: +s.opportunity_score.toFixed(1),
    competition: +s.competition_score.toFixed(1),
  }));
  return (
    <section className="rounded-xl bg-surface border border-border p-4">
      <div className="flex items-center gap-2 mb-3">
        <History size={14} className="text-gray-400" />
        <h3 className="text-xs font-semibold uppercase tracking-wide text-gray-400">
          Research history — "{keyword}" ({h.length} snapshots)
        </h3>
      </div>
      <ResponsiveContainer width="100%" height={180}>
        <LineChart data={data} margin={{ top: 4, right: 8, bottom: 0, left: 0 }}>
          <XAxis dataKey="at" stroke="#4a5568" fontSize={11} />
          <YAxis domain={[0, 100]} stroke="#4a5568" fontSize={11} />
          <Tooltip
            contentStyle={{
              background: '#1d212b',
              border: '1px solid #2a2f3a',
              borderRadius: 8,
              fontSize: 12,
            }}
          />
          <Legend wrapperStyle={{ fontSize: 11 }} />
          <Line
            type="monotone"
            dataKey="opportunity"
            name="Opportunity"
            stroke="#3fbf6f"
            strokeWidth={2}
            dot={{ r: 3 }}
            isAnimationActive={false}
          />
          <Line
            type="monotone"
            dataKey="competition"
            name="Competition"
            stroke="#e05a5a"
            strokeWidth={2}
            dot={{ r: 3 }}
            isAnimationActive={false}
          />
        </LineChart>
      </ResponsiveContainer>
    </section>
  );
}

function ResearchResults({ r }: { r: KeywordResearch }) {
  const oppColor =
    r.opportunity_score >= 70
      ? 'text-green-400'
      : r.opportunity_score >= 40
        ? 'text-yellow-400'
        : 'text-red-400';
  const compColor =
    r.competition_score >= 70
      ? 'text-red-400'
      : r.competition_score >= 40
        ? 'text-yellow-400'
        : 'text-green-400';

  const volumeColor =
    r.volume_label === 'High'
      ? 'text-green-400'
      : r.volume_label === 'Medium'
        ? 'text-yellow-400'
        : r.volume_label === 'Low'
          ? 'text-orange-400'
          : 'text-gray-400';

  return (
    <div className="space-y-4">
      {/* Gauges */}
      <div className="grid grid-cols-2 md:grid-cols-5 gap-3">
        <div className="rounded-xl border border-accent/40 bg-accent/10 p-4">
          <div className="text-xs uppercase tracking-wide text-gray-400">Keyword score</div>
          <div className={`mt-1 text-3xl font-black ${oppColor}`}>{r.keyword_score.toFixed(0)}</div>
          <div className="mt-0.5 text-xs text-gray-500">composite (demand·comp·activity·fit)</div>
        </div>
        <Gauge label="Opportunity" value={r.opportunity_score} color={oppColor} />
        <Gauge label="Competition" value={r.competition_score} color={compColor} />
        <div className="rounded-xl border border-border bg-surface p-4">
          <div className="text-xs uppercase tracking-wide text-gray-500">Search volume</div>
          <div className={`mt-1 text-2xl font-black ${volumeColor}`}>{r.volume_label}</div>
          <div className="mt-0.5 text-xs text-gray-500">
            {r.serp_total} ranking videos · avg {formatViews(r.serp_mean_views)} views
          </div>
        </div>
        <div className="rounded-xl border border-border bg-surface p-4">
          <div className="text-xs uppercase tracking-wide text-gray-500">Activity</div>
          <div className={`mt-1 text-2xl font-black ${r.actively_published ? 'text-green-400' : 'text-gray-400'}`}>
            {r.actively_published ? 'Active' : 'Quiet'}
          </div>
          <div className="mt-0.5 text-xs text-gray-500">
            {r.recent_uploads} new videos in 90d
          </div>
        </div>
      </div>

      {/* Verdict */}
      <div
        className={`rounded-xl p-4 border flex items-start gap-3 ${
          r.opportunity_score >= 70
            ? 'border-green-500/30 bg-green-500/10'
            : r.opportunity_score >= 40
              ? 'border-yellow-500/30 bg-yellow-500/10'
              : 'border-red-500/30 bg-red-500/10'
        }`}
      >
        <Target size={18} className="mt-0.5 shrink-0 text-current" />
        <div className="text-sm">
          <div className="font-semibold mb-0.5">
            {r.opportunity_score >= 70
              ? `Strong opportunity — "${r.keyword}" is underserved`
              : r.opportunity_score >= 40
                ? `Moderate opportunity — "${r.keyword}" has room but competition exists`
                : `Saturated or low demand — "${r.keyword}" needs a sharper angle`}
          </div>
          <div className="text-gray-400">{r.verdict}</div>
        </div>
      </div>

      {/* Suggested tags + related keywords */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        <section className="rounded-xl bg-surface border border-border p-4">
          <div className="flex items-center gap-2 mb-2">
            <TagIcon size={14} className="text-gray-400" />
            <h3 className="text-xs font-semibold uppercase tracking-wide text-gray-400">
              Suggested tags — harvested from ranking videos
            </h3>
          </div>
          <div className="flex flex-wrap gap-1.5">
            {r.suggested_tags.length === 0 ? (
              <span className="text-xs text-gray-500">No tags exposed by ranking videos.</span>
            ) : (
              r.suggested_tags.map((t) => (
                <span
                  key={t.tag}
                  className="px-2 py-1 rounded-full bg-accent/10 border border-accent/25 text-xs text-accent"
                  title={`Used by ${t.usage} of ${r.serp_total} ranking videos`}
                >
                  {t.tag}
                  <span className="ml-1.5 text-[10px] opacity-70">×{t.usage}</span>
                </span>
              ))
            )}
          </div>
        </section>

        <section className="rounded-xl bg-surface border border-border p-4">
          <div className="flex items-center gap-2 mb-2">
            <Hash size={14} className="text-gray-400" />
            <h3 className="text-xs font-semibold uppercase tracking-wide text-gray-400">
              Related keywords (autocomplete, by popularity)
            </h3>
          </div>
          <div className="flex flex-wrap gap-1.5">
            {r.related_keywords.length === 0 ? (
              <span className="text-xs text-gray-500">No suggestions.</span>
            ) : (
              r.related_keywords.slice(0, 12).map((s) => (
                <span
                  key={s.keyword}
                  className="px-2 py-1 rounded-full bg-gray-800/60 border border-border text-xs text-gray-300"
                  title={`Autocomplete popularity rank #${s.popularity_rank}`}
                >
                  <span className="text-[10px] text-gray-500 mr-1">#{s.popularity_rank}</span>
                  {s.keyword}
                </span>
              ))
            )}
          </div>
        </section>
      </div>

      {/* SERP overlay */}
      <section className="rounded-xl bg-surface border border-border overflow-hidden">
        <div className="px-4 py-3 border-b border-border flex items-center gap-2">
          <Users size={14} className="text-gray-400" />
          <h3 className="text-xs font-semibold uppercase tracking-wide text-gray-400">
            Ranking now — full overlay (views · engagement · tags · SEO score)
          </h3>
        </div>
        <div className="divide-y divide-border/50">
          {r.serp.length === 0 ? (
            <div className="px-4 py-6 text-center text-gray-500 text-sm">
              No results — try a broader topic.
            </div>
          ) : (
            r.serp.map((v, i) => (
              <div key={v.video_id} className="px-4 py-3">
                <div className="flex items-center gap-3">
                  <span className="text-xs text-gray-600 w-4 text-right">#{i + 1}</span>
                  <div className="min-w-0 flex-1">
                    <div className="text-sm truncate">{v.title}</div>
                    <div className="text-xs text-gray-500">
                      {v.channel}
                      {v.upload_date && (
                        <span className="ml-2">· {v.upload_date.slice(0, 4)}-{v.upload_date.slice(4, 6)}</span>
                      )}
                    </div>
                  </div>
                  <div className="text-right shrink-0">
                    <div className="text-xs text-gray-400">{formatViews(v.view_count)} views</div>
                    <div className="text-[10px] text-gray-500">
                      {v.like_count != null ? `${formatViews(v.like_count)} 👍` : ''}
                      {v.comment_count != null ? ` · ${formatViews(v.comment_count)} 💬` : ''}
                    </div>
                  </div>
                  <div className="shrink-0 w-14 text-right">
                    <span
                      className={`px-1.5 py-0.5 rounded text-[10px] font-bold ${
                        v.seo_score >= 70
                          ? 'bg-green-500/15 text-green-400'
                          : v.seo_score >= 50
                            ? 'bg-yellow-500/15 text-yellow-400'
                            : 'bg-red-500/15 text-red-400'
                      }`}
                    >
                      SEO {v.seo_score.toFixed(0)}
                    </span>
                  </div>
                </div>
                {v.tags.length > 0 && (
                  <div className="mt-1.5 ml-7 flex flex-wrap gap-1">
                    {v.tags.slice(0, 8).map((t) => (
                      <span
                        key={t}
                        className="px-1.5 py-0.5 rounded bg-gray-800/50 border border-border text-[10px] text-gray-400"
                      >
                        {t}
                      </span>
                    ))}
                    {v.tags.length > 8 && (
                      <span className="text-[10px] text-gray-600">+{v.tags.length - 8}</span>
                    )}
                  </div>
                )}
              </div>
            ))
          )}
        </div>
      </section>

      {/* Our corpus matches */}
      <section className="rounded-xl bg-surface border border-border overflow-hidden">
        <div className="px-4 py-3 border-b border-border flex items-center gap-2">
          <Database size={14} className="text-gray-400" />
          <h3 className="text-xs font-semibold uppercase tracking-wide text-gray-400">
            Our database matches
            {r.corpus_resonance !== null && (
              <span className="ml-2 text-accent">resonance {r.corpus_resonance.toFixed(0)}/100</span>
            )}
          </h3>
        </div>
        {r.corpus_matches.length === 0 ? (
          <div className="px-4 py-5 text-center text-gray-500 text-sm">
            No stored videos match — this topic is fresh for your corpus.
          </div>
        ) : (
          <div className="divide-y divide-border/50">
            {r.corpus_matches.map((m) => (
              <div key={m.video_id} className="px-4 py-2.5 flex items-center gap-3">
                <div className="min-w-0 flex-1">
                  <Link to={`/scores/${m.video_id}`} className="text-sm hover:text-accent truncate block">
                    {m.title}
                  </Link>
                  <div className="text-xs text-gray-500">{m.channel ?? '—'}</div>
                </div>
                <span className="text-xs text-gray-400 whitespace-nowrap">
                  {formatViews(m.view_count)}
                </span>
              </div>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
