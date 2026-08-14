import { Clock } from 'lucide-react';

/// Shows when the underlying data was last computed, and flags it if stale
/// (>30 days old) so the UI never presents old analysis as live.
export function FreshnessBadge({ at }: { at: string | null }) {
  if (!at) return null;
  const t = new Date(at).getTime();
  const ageDays = (Date.now() - t) / (1000 * 60 * 60 * 24);
  const stale = ageDays > 30;

  return (
    <span
      className={`inline-flex items-center gap-1 text-[11px] px-2 py-0.5 rounded-full border ${
        stale
          ? 'text-yellow-400 bg-yellow-500/10 border-yellow-500/30'
          : 'text-gray-400 bg-gray-500/10 border-gray-500/20'
      }`}
      title={stale ? 'Stale — re-run keyword research for current data' : `Analyzed ${new Date(at).toLocaleString()}`}
    >
      <Clock size={10} />
      {stale ? 'Stale data' : new Date(at).toLocaleDateString()}
    </span>
  );
}
