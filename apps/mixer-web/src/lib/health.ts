import { useEffect, useState } from 'react';

export interface Health {
  password_login: boolean;
  lan_urls: string[];
  version: string;
}

/** Lê /api/health (sem token): se há senha e quais os endereços na rede. */
export function useHealth(refreshKey: unknown = 0): Health | null {
  const [h, setH] = useState<Health | null>(null);
  useEffect(() => {
    let alive = true;
    fetch('/api/health')
      .then((r) => r.json())
      .then((j) => alive && setH(j))
      .catch(() => alive && setH(null));
    return () => {
      alive = false;
    };
  }, [refreshKey]);
  return h;
}

/** Endereço para mandar a outra pessoa: o da rede, nunca o 127.0.0.1. */
export function shareBase(h: Health | null): string {
  const local = /^(localhost|127\.|\[::1\])/.test(location.hostname);
  if (!local || !h?.lan_urls.length) return location.origin;
  return h.lan_urls[0];
}
