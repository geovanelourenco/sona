import { useEffect, useState } from 'react';
import { tokenStore, useMixer } from '../lib/store';
import { Console } from './Console';
import { Header, type View } from './Header';
import { Login } from './Login';
import { Routing } from './Routing';
import { SonaMark } from './Brand';
import { cx } from './kit';

function tokenFromHash(): string | null {
  const m = location.hash.match(/token=([0-9a-f]+)/i);
  if (!m) return null;
  history.replaceState(null, '', location.pathname + location.search);
  return m[1];
}

export function App() {
  const conn = useMixer((s) => s.conn);
  const state = useMixer((s) => s.state);
  const connect = useMixer((s) => s.connect);
  const disconnect = useMixer((s) => s.disconnect);
  const toasts = useMixer((s) => s.toasts);
  const [view, setView] = useState<View>('mesa');

  useEffect(() => {
    const t = tokenFromHash() ?? tokenStore.get();
    if (t) {
      tokenStore.set(t);
      connect(t);
    }
    // Link de acesso aberto numa aba que já está no Sona: o hash muda sem recarregar.
    const onHash = () => {
      const h = tokenFromHash();
      if (h) {
        tokenStore.set(h);
        connect(h);
      }
    };
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  }, [connect]);

  const login = (t: string) => {
    tokenStore.set(t);
    connect(t);
  };

  let body;
  if (conn === 'idle' || conn === 'unauthorized') {
    body = <Login onToken={login} error={conn === 'unauthorized' ? 'Código inválido ou revogado.' : undefined} />;
  } else if (!state) {
    body = (
      <main className="grid h-full place-items-center">
        <div className="flex flex-col items-center gap-3 text-subtle">
          <SonaMark size={48} />
          {conn === 'closed' ? 'Servidor fora do ar. Tentando de novo…' : 'Conectando ao servidor…'}
        </div>
      </main>
    );
  } else {
    body = (
      <div className="flex h-full flex-col">
        <Header
          view={view}
          setView={setView}
          onLogout={() => {
            tokenStore.set(null);
            disconnect();
          }}
        />
        {conn !== 'open' && (
          <div role="status" className="shrink-0 bg-warn/15 px-4 py-1.5 text-[12px] font-semibold text-warn-light">
            Sem conexão com o servidor. O áudio continua no servidor; reconectando…
          </div>
        )}
        {view === 'mesa' ? <Console /> : <Routing />}
      </div>
    );
  }

  return (
    <>
      {body}
      <div className="pointer-events-none fixed bottom-4 left-1/2 z-50 flex -translate-x-1/2 flex-col items-center gap-2" aria-live="polite">
        {toasts.map((t) => (
          <div key={t.id} className={cx('rounded-xl border px-4 py-2 text-[12.5px] font-semibold shadow-[var(--shadow-modal)]', t.kind === 'error' ? 'border-warn/50 bg-panel text-warn-light' : 'border-line bg-surface text-text')}>
            {t.text}
          </div>
        ))}
      </div>
    </>
  );
}
