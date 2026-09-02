export type ConnectionStatus = 'connected' | 'connecting' | 'disconnected';

export interface RpcProgress {
  id: string | number;
  progress: number;
  message: string;
}

export interface RpcEvent {
  event: string;
  data: any;
  timestamp: string;
}

class RpcClient {
  status = $state<ConnectionStatus>('disconnected');
  recentEvents = $state<RpcEvent[]>([]);
  activeCalls = $state<Map<string | number, { resolve: (val: any) => void; reject: (err: any) => void }>>(new Map());

  private ws: WebSocket | null = null;
  private reqId = 0;
  private reconnectTimer: any = null;

  constructor() {
    if (typeof window !== 'undefined') {
      this.connect();
    }
  }

  connect() {
    if (this.ws && (this.ws.readyState === WebSocket.OPEN || this.ws.readyState === WebSocket.CONNECTING)) {
      return;
    }

    this.status = 'connecting';
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const host = window.location.host;
    const url = `${protocol}//${host}/ws`;

    try {
      this.ws = new WebSocket(url);

      this.ws.onopen = () => {
        this.status = 'connected';
        if (this.reconnectTimer) {
          clearTimeout(this.reconnectTimer);
          this.reconnectTimer = null;
        }
      };

      this.ws.onclose = () => {
        this.status = 'disconnected';
        this.scheduleReconnect();
      };

      this.ws.onerror = () => {
        this.status = 'disconnected';
      };

      this.ws.onmessage = (e) => {
        try {
          const msg = JSON.parse(e.data);
          if (msg.type === 'result' && msg.id !== undefined) {
            const pending = this.activeCalls.get(msg.id);
            if (pending) {
              pending.resolve(msg.data);
              this.activeCalls.delete(msg.id);
            }
          } else if (msg.type === 'error' && msg.id !== undefined) {
            const pending = this.activeCalls.get(msg.id);
            if (pending) {
              pending.reject(new Error(msg.error?.message || 'RPC Error'));
              this.activeCalls.delete(msg.id);
            }
          } else if (msg.type === 'notification') {
            this.recentEvents = [
              {
                event: msg.event || 'system.event',
                data: msg.data,
                timestamp: new Date().toLocaleTimeString(),
              },
              ...this.recentEvents.slice(0, 49),
            ];
          }
        } catch {
          // ignore non-json frames
        }
      };
    } catch {
      this.status = 'disconnected';
      this.scheduleReconnect();
    }
  }

  private scheduleReconnect() {
    if (this.reconnectTimer) return;
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      this.connect();
    }, 3000);
  }

  async call<T = any>(method: string, params: Record<string, any> = {}): Promise<T> {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      // Fallback to REST HTTP if WS is disconnected
      try {
        const queryParams = new URLSearchParams();
        for (const [k, v] of Object.entries(params)) {
          if (v !== undefined && v !== null) {
            queryParams.set(k, String(v));
          }
        }
        const qs = queryParams.toString() ? `?${queryParams.toString()}` : '';
        const res = await fetch(`/api/${method.replace('.', '/')}${qs}`);
        if (res.ok) {
          return await res.json();
        }
      } catch {
        // proceed to reject
      }
      throw new Error(`WebSocket disconnected and REST fallback failed for ${method}`);
    }

    const id = `req-${++this.reqId}`;
    const payload = JSON.stringify({ id, method, params });

    return new Promise<T>((resolve, reject) => {
      this.activeCalls.set(id, { resolve, reject });
      this.ws!.send(payload);

      // 30s timeout guard
      setTimeout(() => {
        if (this.activeCalls.has(id)) {
          this.activeCalls.delete(id);
          reject(new Error(`RPC timeout (30s) on method: ${method}`));
        }
      }, 30000);
    });
  }
}

export const rpc = new RpcClient();
