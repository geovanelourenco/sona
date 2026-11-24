import { useCallback, useEffect, useState } from 'react';
import { getAccess, login, type Access } from '../lib/access';
import { tokenStore, useMixer } from '../lib/store';
import { SonaMark } from './Brand';
import { Console } from './Console';
import { Header, type View } from './Header';
import { Login } from './Login';
import { Routing } from './Routing';
import { Settings } from './Settings';
import { Setup } from './Setup';
import { cx } from './kit';

type Screen = 'boot' | 'offline' | 'setup' | 'login' | 'console';

export function App() {
  const conn = useMixer((s) => s.conn);
  const state = useMixer((s) => s.state);
  const connect = useMixer((s) => s.connect);
  const disconnect = useMixer((s) => s.disconnect);
  const toasts = useMixer((s) => s.toasts);
  const loginNotice = useMixer((s) => s.loginNotice);
  const [screen, setScreen] = useState<Screen>('boot');
  const [access, setAccess] = useState<Access | null>(null);
  const [view, setView] = useState<View>('mesa');
  const [settings, setSettings] = useState(false);

  const enter = useCallback(
    (t: string) => {
      tokenStore.set(t);
      connect(t);
      setScreen('console');
    },
    [connect],
  );

  /** Decide a tela: sessão guardada, entrada direta neste computador, primeira configuração ou senha. */
  const boot = useCallback(async () => {
    let acc: Access;
    try {
      acc = await getAccess();
    } catch {
      setScreen('offline');
      setTimeout(boot, 2000);
      return;
    }
    setAccess(acc);
    if (acc.local && !acc.app_password) {
      // Neste computador sem senha do app: entra direto.
      const t = await login('').catch(() => null);
      if (!t) return setScreen('login');
      tokenStore.set(t);
      if (acc.setup_needed) return setScreen('setup');
      return enter(t);
    }
    const saved = tokenStore.get();
    if (saved) return enter(saved);
    setScreen('login');
  }, [enter]);

  useEffect(() => {
    boot();
  }, [boot]);

  // Sessão recusada ou encerrada (senha trocada): volta para a decisão de tela.
  useEffect(() => {
    if (screen === 'console' && (conn === 'unauthorized' || conn === 'idle')) {
      tokenStore.set(null);
      boot();
    }
  }, [conn, screen, boot]);

  let body;
  if (screen === 'setup') {
    body = <Setup onDone={() => enter(tokenStore.get()!)} />;
  } else if (screen === 'login') {
    body = <Login access={access} onToken={enter} notice={loginNotice} />;
  } else if (screen === 'console' && state) {
    body = (
      <div className="flex h-full flex-col">
        <Header
          view={view}
          setView={setView}
          onSettings={() => setSettings(true)}
          onLogout={
            access?.local && !access.app_password
              ? undefined
              : () => {
                  tokenStore.set(null);
                  disconnect();
                }
          }
        />
        {conn !== 'open' && (
          <div role="status" className="shrink-0 bg-warn/15 px-4 py-1.5 text-[12px] font-semibold text-warn-light">
            Sem conexão com o servidor. O áudio continua no servidor; reconectando…
          </div>
        )}
        {view === 'mesa' ? <Console /> : <Routing />}
        {settings && (
          <Settings
            onClose={() => {
              setSettings(false);
              getAccess().then(setAccess).catch(() => {});
            }}
          />
        )}
      </div>
    );
  } else {
    body = (
      <main className="grid h-full place-items-center">
        <div className="flex flex-col items-center gap-3 text-subtle">
          <SonaMark size={48} />
          {screen === 'offline' || conn === 'closed' ? 'Servidor fora do ar. Tentando de novo…' : 'Conectando ao servidor…'}
        </div>
      </main>
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
