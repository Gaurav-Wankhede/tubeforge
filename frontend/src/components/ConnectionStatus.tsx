import { Wifi, WifiOff } from 'lucide-react';
import { useRpc } from '../lib/rpc';

/// Live connection indicator — green when WebSocket is connected, red otherwise.
export function ConnectionStatus() {
  const { connected } = useRpc();

  return (
    <div className="flex items-center gap-1.5 text-xs">
      {connected ? (
        <>
          <Wifi size={12} className="text-green-400" />
          <span className="text-green-400">Live</span>
        </>
      ) : (
        <>
          <WifiOff size={12} className="text-red-400" />
          <span className="text-red-400">Offline</span>
        </>
      )}
    </div>
  );
}
