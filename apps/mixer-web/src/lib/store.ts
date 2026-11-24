import { create } from 'zustand';
import { setMeters } from './meters';
import type { EngineStatus, MixerConfig, Role, ServerState } from './types';

type Conn = 'idle' | 'connecting' | 'open' | 'closed' | 'unauthorized';

interface Toast {
  id: number;
  kind: 'error' | 'info';
  text: string;
}

interface Store {
  conn: Conn;
  role: Role | null;
  name: string;
  session: number;
  state: ServerState | null;
  engine: EngineStatus | null;
  toasts: Toast[];
  /** Bus de fone deste operador (índice), guardado neste navegador. */
  myMonitor: number | null;
  connect: (token: string) => void;
  disconnect: () => void;
  patch: (path: string, value: unknown) => void;
  send: (msg: object) => void;
  setMyMonitor: (b: number | null) => void;
  toast: (kind: Toast['kind'], text: string) => void;
}

const LS = {
  get: (k: string) => {
    try {
      return localStorage.getItem(k);
    } catch {
      return null;
    }
  },
  set: (k: string, v: string | null) => {
    try {
      if (v === null) localStorage.removeItem(k);
      else localStorage.setItem(k, v);
    } catch {
      /* navegador sem storage: segue sem lembrar */
    }
  },
};

export const tokenStore = {
  get: () => LS.get('sona.token'),
  set: (t: string | null) => LS.set('sona.token', t),
};

let ws: WebSocket | null = null;
let retry = 0;
let retryTimer: ReturnType<typeof setTimeout> | undefined;
let wantToken: string | null = null;

// Patches pendentes, agrupados por caminho, enviados a cada 40 ms (fader
// arrastado não inunda o servidor).
const pending = new Map<string, unknown>();
let flushTimer: ReturnType<typeof setTimeout> | undefined;

function mergePatch(target: unknown, patch: unknown): unknown {
  if (patch === null || typeof patch !== 'object' || Array.isArray(patch)) return patch;
  const t: Record<string, unknown> = target && typeof target === 'object' && !Array.isArray(target) ? { ...(target as Record<string, unknown>) } : {};
  for (const [k, v] of Object.entries(patch as Record<string, unknown>)) {
    if (v === null) delete t[k];
    else t[k] = mergePatch(t[k], v);
  }
  return t;
}

export function applyAt(doc: MixerConfig, path: string, value: unknown): MixerConfig {
  const parts = path.split('/').filter(Boolean);
  const root = structuredClone(doc) as unknown as Record<string, unknown>;
  if (!parts.length) return mergePatch(root, value) as MixerConfig;
  let node: Record<string, unknown> = root;
  for (let i = 0; i < parts.length - 1; i++) node = node[parts[i]] as Record<string, unknown>;
  const last = parts[parts.length - 1];
  node[last] = mergePatch(node[last], value);
  return root as unknown as MixerConfig;
}

export const useMixer = create<Store>((set, get) => ({
  conn: 'idle',
  role: null,
  name: '',
  session: 0,
  state: null,
  engine: null,
  toasts: [],
  myMonitor: (() => {
    const v = LS.get('sona.monitor');
    return v === null ? null : Number(v);
  })(),

  connect(token) {
    wantToken = token;
    clearTimeout(retryTimer);
    ws?.close();
    set({ conn: 'connecting' });
    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    const sock = new WebSocket(`${proto}://${location.host}/api/ws?token=${encodeURIComponent(token)}`);
    ws = sock;
    let opened = false;
    sock.onopen = () => {
      opened = true;
      retry = 0;
    };
    sock.onmessage = (e) => {
      const m = JSON.parse(e.data);
      switch (m.type) {
        case 'hello':
          set({ conn: 'open', role: m.role, name: m.name, session: m.session, state: m.state, engine: m.engine });
          break;
        case 'state':
          // Eco do próprio patch com fila ainda pendente: espera a fila esvaziar.
          if (m.by === get().session && pending.size) return;
          set({ state: m.state });
          break;
        case 'meters':
          setMeters(m);
          break;
        case 'engine':
          set({ engine: m.engine });
          break;
        case 'error':
          get().toast('error', m.message);
          break;
      }
    };
    sock.onclose = () => {
      if (ws !== sock) return;
      if (!opened && retry === 0) {
        // Recusado no aperto de mão: quase sempre token inválido.
        fetch('/api/state', { headers: { Authorization: `Bearer ${token}` } })
          .then((r) => set({ conn: r.status === 401 ? 'unauthorized' : 'closed' }))
          .catch(() => set({ conn: 'closed' }));
      } else {
        set({ conn: 'closed' });
      }
      if (wantToken) {
        retry = Math.min(retry + 1, 6);
        retryTimer = setTimeout(() => wantToken && get().conn !== 'unauthorized' && get().connect(wantToken), 400 * 2 ** retry);
      }
    };
  },

  disconnect() {
    wantToken = null;
    clearTimeout(retryTimer);
    ws?.close();
    ws = null;
    set({ conn: 'idle', state: null, role: null });
  },

  send(msg) {
    if (ws?.readyState === WebSocket.OPEN) ws.send(JSON.stringify(msg));
  },

  patch(path, value) {
    const st = get().state;
    if (!st) return;
    set({ state: { ...st, mixer: applyAt(st.mixer, path, value) } });
    pending.set(path, mergePatch(pending.get(path), value));
    if (!flushTimer)
      flushTimer = setTimeout(() => {
        flushTimer = undefined;
        for (const [p, v] of pending) get().send({ type: 'patch', path: p, value: v });
        pending.clear();
      }, 40);
  },

  setMyMonitor(b) {
    LS.set('sona.monitor', b === null ? null : String(b));
    set({ myMonitor: b });
  },

  toast(kind, text) {
    const id = Date.now() + Math.random();
    set((s) => ({ toasts: [...s.toasts.slice(-3), { id, kind, text }] }));
    setTimeout(() => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })), kind === 'error' ? 6000 : 3000);
  },
}));
