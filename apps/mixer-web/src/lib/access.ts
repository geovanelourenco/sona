import { tokenStore } from './store';

export interface Access {
  /** O navegador está no computador do servidor. */
  local: boolean;
  setup_needed: boolean;
  app_password: boolean;
  remote_enabled: boolean;
  lan_urls: string[];
}

export async function getAccess(): Promise<Access> {
  const r = await fetch('/api/access');
  if (!r.ok) throw new Error('Servidor fora do ar.');
  return r.json();
}

/** Entra e devolve o token. Neste computador sem senha do app, `password` vazio basta. */
export async function login(password: string, name = ''): Promise<string> {
  const r = await fetch('/api/login', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ password, name }) });
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(j.error ?? 'Não consegui entrar.');
  return j.token;
}

/** Muda senhas. Campo ausente = sem mudança; null = apagar. */
export async function updateAccess(change: { app_password?: string | null; remote_password?: string | null; finish_setup?: boolean }) {
  const r = await fetch('/api/access', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${tokenStore.get()}` },
    body: JSON.stringify(change),
  });
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(j.error ?? 'Não consegui salvar.');
  return j as { app_password: boolean; remote_enabled: boolean; setup_done: boolean };
}
