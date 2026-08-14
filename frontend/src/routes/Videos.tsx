import { useState, useCallback, useEffect } from 'react';
import { Link } from 'react-router-dom';
import { useRpc } from '../lib/rpc';
import type { Video } from '../lib/types';
import { Film, Search, Loader2 } from 'lucide-react';

type VideosResult = {
  items: Video[];
  total: number;
  page: number;
  page_size: number;
};

function formatViews(v: number | null) {
  if (v == null) return '—';
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
  if (v >= 1_000) return `${(v / 1_000).toFixed(0)}k`;
  return String(v);
}

export default function Videos() {
  const { call, connected } = useRpc();
  const [data, setData] = useState<VideosResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [q, setQ] = useState('');
  const [page, setPage] = useState(1);

  const fetchVideos = useCallback(async () => {
    if (!connected) return;
    setLoading(true);
    setError(null);
    try {
      const result = (await call('videos.list', { q, page, page_size: 20 })) as VideosResult;
      setData(result);
    } catch (e) {
      setError((e as Error).message || 'Failed to load videos');
    } finally {
      setLoading(false);
    }
  }, [call, connected, q, page]);

  useEffect(() => {
    fetchVideos();
  }, [fetchVideos]);

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold">Videos</h1>
        <button
          onClick={fetchVideos}
          disabled={loading || !connected}
          className="text-xs text-accent hover:underline disabled:opacity-50"
        >
          {loading ? 'Loading...' : 'Refresh'}
        </button>
      </div>

      {error && (
        <div className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-400">
          {error}
        </div>
      )}

      {/* Search */}
      <div className="relative">
        <Search size={16} className="absolute left-3 top-1/2 -translate-y-1/2 text-gray-500" />
        <input
          type="text"
          placeholder="Filter by title..."
          value={q}
          onChange={(e) => { setQ(e.target.value); setPage(1); }}
          className="w-full pl-9 pr-3 py-2.5 bg-surface border border-border rounded-lg text-sm focus:outline-none focus:border-accent"
        />
      </div>

      {loading ? (
        <div className="flex justify-center py-8 text-gray-500">
          <Loader2 size={20} className="animate-spin mr-2" /> Loading videos...
        </div>
      ) : !data || data.items.length === 0 ? (
        <div className="rounded-xl bg-surface border border-border p-12 text-center text-gray-500">
          <Film size={32} className="mx-auto mb-3 opacity-50" />
          No videos found — run <code className="text-gray-400">tubeforge ingest</code>.
        </div>
      ) : (
        <>
          <div className="rounded-xl bg-surface border border-border overflow-hidden">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-border">
                  <th className="px-4 py-3 text-left font-medium text-gray-400">Title</th>
                  <th className="px-4 py-3 text-right font-medium text-gray-400">Views</th>
                  <th className="px-4 py-3 text-right font-medium text-gray-400">Likes</th>
                  <th className="px-4 py-3 text-right font-medium text-gray-400">Comments</th>
                </tr>
              </thead>
              <tbody>
                {data.items.map((v: Video) => (
                  <tr key={v.video_id} className="border-b border-border/50 hover:bg-surface-hover">
                    <td className="px-4 py-3">
                      <Link to={`/scores/${v.video_id}`} className="hover:text-accent">
                        {v.title}
                      </Link>
                    </td>
                    <td className="px-4 py-3 text-right text-gray-400">{formatViews(v.view_count)}</td>
                    <td className="px-4 py-3 text-right text-gray-400">{formatViews(v.like_count)}</td>
                    <td className="px-4 py-3 text-right text-gray-400">{formatViews(v.comment_count)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          {/* Pagination */}
          {data.total > data.page_size && (
            <div className="flex items-center justify-between text-sm text-gray-400">
              <span>Showing {data.items.length} of {data.total}</span>
              <div className="flex gap-2">
                <button
                  onClick={() => setPage(Math.max(1, page - 1))}
                  disabled={page <= 1}
                  className="px-3 py-1 rounded border border-border hover:bg-surface-hover disabled:opacity-50"
                >
                  Prev
                </button>
                <button
                  onClick={() => setPage(page + 1)}
                  disabled={page * data.page_size >= data.total}
                  className="px-3 py-1 rounded border border-border hover:bg-surface-hover disabled:opacity-50"
                >
                  Next
                </button>
              </div>
            </div>
          )}
        </>
      )}
    </div>
  );
}
