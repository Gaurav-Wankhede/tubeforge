import { useCallback, useEffect, useRef, useState } from 'react';

type RpcRequest = {
  id: string;
  method: string;
  params?: Record<string, unknown>;
};

type RpcResponse =
  | { type: 'progress'; id: string; progress: number; message: string }
  | { type: 'result'; id: string; data: unknown }
  | { type: 'error'; id: string; error: { code: number; message: string } }
  | { type: 'notification'; event: string; data: unknown };

type PendingRequest = {
  resolve: (data: unknown) => void;
  reject: (err: Error) => void;
  onProgress?: (progress: number, message: string) => void;
  timeout: number;
};

type RpcOptions = {
  url?: string;
  onNotification?: (event: string, data: unknown) => void;
  reconnectInterval?: number;
  /** Timeout (ms) before a request is rejected as failed. */
  timeout?: number;
};

const DEFAULT_TIMEOUT = 30_000;

/// Reject every pending request — used on reconnect/close so no caller hangs.
function flushPending(
  pending: Map<string, PendingRequest>,
  reason: string,
) {
  for (const [, p] of pending) {
    clearTimeout(p.timeout);
    p.reject(new Error(reason));
  }
  pending.clear();
}

export function useRpc(options: RpcOptions = {}) {
  const {
    url = `ws://${window.location.host}/ws`,
    onNotification,
    reconnectInterval = 3000,
    timeout = DEFAULT_TIMEOUT,
  } = options;

  const wsRef = useRef<WebSocket | null>(null);
  const pendingRef = useRef<Map<string, PendingRequest>>(new Map());
  const idCounterRef = useRef(0);
  const [connected, setConnected] = useState(false);

  const connect = useCallback(() => {
    // Reject anything still pending from a previous session.
    flushPending(pendingRef.current, 'Connection reset — request aborted');

    const ws = new WebSocket(url);
    wsRef.current = ws;

    ws.onopen = () => setConnected(true);
    ws.onclose = () => {
      setConnected(false);
      setTimeout(connect, reconnectInterval);
    };
    ws.onerror = () => ws.close();

    ws.onmessage = (event) => {
      let res: RpcResponse;
      try {
        res = JSON.parse(event.data);
      } catch {
        return;
      }

      if (res.type === 'notification') {
        onNotification?.(res.event, res.data);
        return;
      }

      const pending = pendingRef.current.get(res.id);
      if (!pending) return;

      switch (res.type) {
        case 'progress':
          pending.onProgress?.(res.progress, res.message);
          break;
        case 'result':
          clearTimeout(pending.timeout);
          pending.resolve(res.data);
          pendingRef.current.delete(res.id);
          break;
        case 'error':
          clearTimeout(pending.timeout);
          pending.reject(new Error(res.error.message));
          pendingRef.current.delete(res.id);
          break;
      }
    };
  }, [url, onNotification, reconnectInterval]);

  useEffect(() => {
    connect();
    const ws = wsRef.current;
    const pending = pendingRef.current;
    return () => {
      flushPending(pending, 'Disconnected');
      ws?.close();
    };
  }, [connect]);

  const call = useCallback(
    (
      method: string,
      params?: Record<string, unknown>,
      onProgress?: (progress: number, message: string) => void,
      requestTimeout?: number,
    ) =>
      new Promise<unknown>((resolve, reject) => {
        // Reject immediately if the socket isn't open — avoids hanging callers.
        const ws = wsRef.current;
        if (!ws || ws.readyState !== WebSocket.OPEN) {
          reject(new Error('Not connected to server'));
          return;
        }

        const id = `req-${++idCounterRef.current}`;
        const req: RpcRequest = { id, method, params };
        const effTimeout = requestTimeout ?? timeout;
        const timer = window.setTimeout(() => {
          const p = pendingRef.current.get(id);
          if (p) {
            pendingRef.current.delete(id);
            p.reject(new Error(`Request timed out (${method})`));
          }
        }, effTimeout);

        pendingRef.current.set(id, { resolve, reject, onProgress, timeout: timer });
        ws.send(JSON.stringify(req));
      }),
    [timeout],
  );

  return { call, connected };
}
