// Player multitrack: tipos (espelho de sona-server/src/player) e chamadas HTTP.
import { tokenStore } from './store';

export interface PlayerTrack {
  file: string;
  name: string;
  cue: boolean;
  gain_db: number;
  mute: boolean;
  solo: boolean;
  channels: number;
  sample_rate: number;
  frames: number;
}

export interface PlayerSong {
  id: string;
  name: string;
  bpm: number | null;
  key: string;
  seconds: number;
  tracks: PlayerTrack[];
}

export interface PlayerState {
  songs: PlayerSong[];
  mode: 'simple' | 'split';
  master_db: number;
  stop_at_end: boolean;
  current: string | null;
  playing: boolean;
  position: number;
  underruns: number;
  errors: string[];
}

export interface PlayerTick {
  position: number;
  playing: boolean;
  peaks: number[];
}

export const AUDIO_EXT = ['wav', 'wave', 'aif', 'aiff', 'flac', 'mp3'];
export const isAudioFile = (name: string) => AUDIO_EXT.includes(name.split('.').pop()?.toLowerCase() ?? '');

const auth = (): Record<string, string> => {
  const t = tokenStore.get();
  return t ? { Authorization: `Bearer ${t}` } : {};
};

async function call<T = unknown>(method: string, url: string, body?: unknown): Promise<T> {
  const r = await fetch(url, {
    method,
    headers: { ...auth(), ...(body !== undefined ? { 'Content-Type': 'application/json' } : {}) },
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error((j as { error?: string }).error ?? `Erro ${r.status}`);
  return j as T;
}

export const playerApi = {
  action: (action: string, extra: Record<string, unknown> = {}) => call<PlayerState>('POST', '/api/player', { action, ...extra }),
  create: (name: string) => call<{ id: string }>('POST', '/api/player/songs', { name }),
  update: (id: string, patch: Record<string, unknown>) => call('POST', `/api/player/songs/${encodeURIComponent(id)}`, patch),
  remove: (id: string) => call('DELETE', `/api/player/songs/${encodeURIComponent(id)}`),
  removeTrack: (id: string, file: string) => call('DELETE', `/api/player/songs/${encodeURIComponent(id)}/files/${encodeURIComponent(file)}`),
  /** Envia um arquivo cru; `onProgress` recebe bytes enviados. */
  upload(id: string, file: File, onProgress: (sent: number) => void): Promise<void> {
    return new Promise((resolve, reject) => {
      const x = new XMLHttpRequest();
      x.open('PUT', `/api/player/songs/${encodeURIComponent(id)}/files/${encodeURIComponent(file.name)}`);
      for (const [k, v] of Object.entries(auth())) x.setRequestHeader(k, v);
      x.upload.onprogress = (e) => onProgress(e.loaded);
      x.onload = () => {
        if (x.status >= 200 && x.status < 300) return resolve();
        let msg = `Erro ${x.status}`;
        try {
          msg = JSON.parse(x.responseText).error ?? msg;
        } catch {
          /* resposta sem JSON */
        }
        reject(new Error(`${file.name}: ${msg}`));
      };
      x.onerror = () => reject(new Error(`${file.name}: conexão caiu durante o envio.`));
      x.send(file);
    });
  },
};

export const fmtTime = (s: number) => {
  const t = Math.max(0, Math.floor(s));
  return `${Math.floor(t / 60)}:${String(t % 60).padStart(2, '0')}`;
};

export const fmtDb = (db: number) => (db <= -60 ? '−∞' : `${db > 0 ? '+' : db < 0 ? '−' : ''}${Math.abs(db).toFixed(1).replace('.', ',')} dB`);
