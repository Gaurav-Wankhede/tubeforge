import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
  CartesianGrid,
  Cell,
} from 'recharts'
import { api } from '../lib/api'
import {
  Search,
  Loader2,
  Target,
  Copy,
  Check,
  TrendingUp,
  TrendingDown,
  Minus,
  Sparkles,
} from 'lucide-react'

interface TopicAnalysis {
  topic: string
  verdict: string
  scores: { opportunity: number; competition: number; keyword_score: number }
  volume: string
  demand: { serp_total: number; avg_views_per_ranking_video: number; actively_published: boolean }
  gap: { score: number; type: string; demand_views: number; supply_videos: number }
  ranking_chart: { position: number; title: string; channel: string; views: number; seo_score: number }[]
  packaging: { title: string; description: string; tags: string[] }
  suggested_tags: { tag: string; usage: number }[]
  related_keywords: { keyword: string; popularity_rank: number }[]
}

function fmtViews(v: number) {
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`
  if (v >= 1_000) return `${(v / 1_000).toFixed(0)}k`
  return String(v)
}

export default function TopicResearch() {
  const [input, setInput] = useState('')
  const [topic, setTopic] = useState('')
  const [copied, setCopied] = useState(false)

  const { data, isLoading, error } = useQuery({
    queryKey: ['topicAnalysis', topic],
    queryFn: () => api.analysisTopic(topic),
    enabled: topic.length > 0,
  })

  const analysis = data as TopicAnalysis | undefined

  const runAnalysis = () => {
    const t = input.trim()
    if (t) setTopic(t)
  }

  const copyAll = async () => {
    if (!analysis) return
    const bundle = [
      `TITLE: ${analysis.packaging.title}`,
      '',
      'DESCRIPTION:',
      analysis.packaging.description,
      '',
      'TAGS:',
      analysis.packaging.tags.join(', '),
    ].join('\n')
    await navigator.clipboard.writeText(bundle)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  const chartData = (analysis?.ranking_chart ?? []).map((r) => ({
    name: `#${r.position} ${r.channel}`.slice(0, 22),
    views: r.views,
    seo: Math.round(r.seo_score),
  }))

  const verdictIcon = (v: string) => {
    if (v === 'rising') return <TrendingUp size={14} className="text-green-400" />
    if (v === 'falling') return <TrendingDown size={14} className="text-red-400" />
    return <Minus size={14} className="text-gray-400" />
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center gap-2">
        <Sparkles size={20} className="text-accent" />
        <h1 className="text-2xl font-bold">Topic Research</h1>
      </div>
      <p className="text-sm text-gray-500 -mt-3">
        Enter a topic you want to make a video about. TubeForge scans YouTube in realtime, finds the
        demand-supply gap, and drafts precise title/description/tags to help you rank.
      </p>

      {/* Topic input */}
      <div className="flex gap-2">
        <div className="relative flex-1">
          <Search size={16} className="absolute left-3 top-1/2 -translate-y-1/2 text-gray-500" />
          <input
            type="text"
            placeholder="e.g. rust async tokio, system design load balancer, ..."
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && runAnalysis()}
            className="w-full pl-9 pr-3 py-2.5 bg-surface border border-border rounded-lg text-sm focus:outline-none focus:border-accent"
          />
        </div>
        <button
          onClick={runAnalysis}
          disabled={isLoading || !input.trim()}
          className="px-4 py-2.5 rounded-lg bg-accent/15 border border-accent/30 text-sm font-semibold text-accent hover:bg-accent/25 disabled:opacity-50 transition-colors"
        >
          {isLoading ? <Loader2 size={14} className="animate-spin inline" /> : 'Analyze'}
        </button>
      </div>

      {isLoading && (
        <div className="p-12 flex justify-center text-gray-500">
          <Loader2 size={20} className="animate-spin mr-2" /> Scanning YouTube for "{topic}"...
        </div>
      )}

      {error && (
        <div className="rounded-xl bg-red-500/10 border border-red-500/30 p-4 text-sm text-red-400">
          Analysis failed. Make sure yt-dlp is enabled (TUBEFORGE_YTDLP_ENABLED=true).
        </div>
      )}

      {analysis && (
        <>
          {/* Verdict + scores */}
          <div className={`rounded-xl p-4 border flex items-start gap-3 ${
            analysis.scores.opportunity >= 70 ? 'border-green-500/30 bg-green-500/10' :
            analysis.scores.opportunity >= 40 ? 'border-yellow-500/30 bg-yellow-500/10' :
            'border-border bg-surface'
          }`}>
            <Target size={18} className="mt-0.5 shrink-0 text-accent" />
            <div className="text-sm">
              <div className="font-semibold capitalize flex items-center gap-1.5">
                {verdictIcon(analysis.verdict)} {analysis.verdict} demand
              </div>
              <div className="text-gray-400 mt-1">{analysis.gap.type}</div>
            </div>
            <div className="ml-auto flex gap-4 text-xs">
              <div className="text-center">
                <div className="text-gray-500">Opportunity</div>
                <div className="font-bold text-lg">{analysis.scores.opportunity.toFixed(0)}</div>
              </div>
              <div className="text-center">
                <div className="text-gray-500">Competition</div>
                <div className="font-bold text-lg">{analysis.scores.competition.toFixed(0)}</div>
              </div>
              <div className="text-center">
                <div className="text-gray-500">Volume</div>
                <div className="font-bold text-lg capitalize">{analysis.volume.toLowerCase()}</div>
              </div>
            </div>
          </div>

          {/* Ranking chart */}
          <div className="rounded-xl bg-surface border border-border p-4">
            <h2 className="text-sm font-medium text-gray-400 mb-3">What ranks now (top videos by views)</h2>
            <ResponsiveContainer width="100%" height={240}>
              <BarChart data={chartData} layout="vertical" margin={{ top: 4, right: 8, bottom: 0, left: 8 }}>
                <CartesianGrid stroke="#2a2f3a" horizontal={false} />
                <XAxis type="number" tick={{ fill: '#9ca3af', fontSize: 11 }} tickFormatter={(v: number) => fmtViews(v)} />
                <YAxis type="category" dataKey="name" tick={{ fill: '#9ca3af', fontSize: 10 }} width={120} />
                <Tooltip contentStyle={{ background: '#1a1a1a', border: '1px solid #333', borderRadius: 8, color: '#fff' }} />
                <Bar dataKey="views" radius={[0, 3, 3, 0]}>
                  {chartData.map((_, i) => (
                    <Cell key={i} fill={i === 0 ? '#4f8cff' : '#3b82f6'} />
                  ))}
                </Bar>
              </BarChart>
            </ResponsiveContainer>
          </div>

          {/* Packaging */}
          <div className="rounded-xl bg-surface border border-border p-4">
            <div className="flex items-center justify-between mb-3">
              <h2 className="text-sm font-medium text-gray-400">Recommended packaging for your video</h2>
              <button
                onClick={copyAll}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-surface border border-border text-xs font-medium hover:border-accent/50 hover:text-gray-200 transition-colors"
              >
                {copied ? <Check size={14} className="text-green-400" /> : <Copy size={14} />}
                {copied ? 'Copied' : 'Copy all'}
              </button>
            </div>
            <div className="text-lg font-bold text-accent mb-3">{analysis.packaging.title}</div>
            <p className="text-sm text-gray-300 whitespace-pre-wrap mb-4">{analysis.packaging.description}</p>
            <div className="flex flex-wrap gap-1.5">
              {analysis.packaging.tags.map((t) => (
                <span key={t} className="px-2 py-1 rounded-full bg-accent/10 border border-accent/25 text-xs text-accent">{t}</span>
              ))}
            </div>
          </div>

          {/* Suggested + related */}
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div className="rounded-xl bg-surface border border-border p-4">
              <h2 className="text-sm font-medium text-gray-400 mb-3">Tags harvested from ranking videos</h2>
              <div className="flex flex-wrap gap-1.5">
                {analysis.suggested_tags.map((t) => (
                  <span key={t.tag} className="px-2 py-1 rounded-full bg-gray-800/60 border border-border text-xs text-gray-300" title={`Used by ${t.usage} ranking videos`}>
                    {t.tag}
                    <span className="ml-1 text-[10px] text-gray-500">×{t.usage}</span>
                  </span>
                ))}
              </div>
            </div>
            <div className="rounded-xl bg-surface border border-border p-4">
              <h2 className="text-sm font-medium text-gray-400 mb-3">Related keywords (autocomplete)</h2>
              <div className="flex flex-wrap gap-1.5">
                {analysis.related_keywords.slice(0, 15).map((r) => (
                  <button
                    key={r.keyword}
                    onClick={() => { setInput(r.keyword); setTopic(r.keyword) }}
                    className="px-2 py-1 rounded-full bg-gray-800/60 border border-border text-xs text-gray-300 hover:border-accent/40 transition-colors"
                  >
                    {r.keyword}
                  </button>
                ))}
              </div>
            </div>
          </div>
        </>
      )}
    </div>
  )
}
