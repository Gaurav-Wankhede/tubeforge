import { useCallback, useEffect, useState } from 'react';
import { useRpc } from '../lib/rpc';
import type { Alert } from '../lib/types';
import { Bell, Check, Trash2 } from 'lucide-react';

const severityColor: Record<string, string> = {
  info: 'border-l-blue-500',
  warning: 'border-l-yellow-500',
  critical: 'border-l-red-500',
};

export default function Alerts() {
  const { call, connected } = useRpc();
  const [alerts, setAlerts] = useState<Alert[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchAlerts = useCallback(async () => {
    if (!connected) return;
    setLoading(true);
    setError(null);
    try {
      const result = (await call('alerts.list')) as { alerts: Alert[] };
      setAlerts(result.alerts);
    } catch (e) {
      setError((e as Error).message || 'Failed to load alerts');
    } finally {
      setLoading(false);
    }
  }, [call, connected]);

  useEffect(() => {
    fetchAlerts();
  }, [fetchAlerts]);

  const handleMarkRead = async () => {
    // Still uses HTTP for mutations (no RPC mutation endpoint yet)
    await fetch('/api/alerts/read', { method: 'POST' });
    fetchAlerts();
  };

  const handleClear = async () => {
    await fetch('/api/alerts/clear', { method: 'POST' });
    fetchAlerts();
  };

  return (
    <div className="space-y-4">
      {error && (
        <div className="rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-xs text-red-400">
          {error}
        </div>
      )}

      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold">Alerts</h1>
        <div className="flex gap-2">
          <button
            onClick={handleMarkRead}
            className="flex items-center gap-1.5 px-3 py-1.5 text-xs bg-surface border border-border rounded-lg hover:bg-surface-hover transition-colors"
          >
            <Check size={14} /> Mark all read
          </button>
          <button
            onClick={handleClear}
            className="flex items-center gap-1.5 px-3 py-1.5 text-xs bg-red-500/10 border border-red-500/20 text-red-400 rounded-lg hover:bg-red-500/20 transition-colors"
          >
            <Trash2 size={14} /> Clear all
          </button>
        </div>
      </div>

      {!loading && alerts.length === 0 && (
        <div className="rounded-xl bg-surface border border-border p-12 text-center text-gray-500">
          <Bell size={32} className="mx-auto mb-3 opacity-50" />
          No alerts
        </div>
      )}

      <div className="space-y-2">
        {alerts.map((alert: Alert) => (
          <div
            key={alert.id}
            className={`rounded-lg bg-surface border border-border border-l-4 p-4 ${
              severityColor[alert.severity] || 'border-l-gray-500'
            } ${alert.read ? 'opacity-60' : ''}`}
          >
            <div className="flex items-start justify-between">
              <div>
                <span className="text-xs font-medium uppercase text-gray-400">
                  {alert.kind}
                </span>
                <p className="mt-1 text-sm">{alert.message}</p>
              </div>
              <span className="text-xs text-gray-500 whitespace-nowrap ml-4">
                {new Date(alert.created_at).toLocaleDateString()}
              </span>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
