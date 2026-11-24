import { useState } from 'react';
import { login, type Access } from '../lib/access';
import { SonaMark } from './Brand';
import { Btn } from './kit';

const LS_NAME = 'sona.name';

/** Entrada com senha: do app (neste computador) ou de acesso remoto (pela rede). */
export function Login({ access, onToken, notice }: { access: Access | null; onToken: (t: string) => void; notice?: string | null }) {
  const remote = access ? !access.local : false;
  const [name, setName] = useState(() => {
    try {
      return localStorage.getItem(LS_NAME) ?? '';
    } catch {
      return '';
    }
  });
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [fail, setFail] = useState<string | null>(null);
  const blocked = remote && access && !access.remote_enabled;

  const shown = fail ?? notice;
  return (
    <main className="grid h-full place-items-center p-4">
      <form
        className="flex w-full max-w-sm flex-col items-center gap-5 rounded-[18px] border border-line bg-panel/80 p-8 shadow-[var(--shadow-modal)]"
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          setFail(null);
          try {
            const t = await login(password, remote ? name.trim() : '');
            try {
              localStorage.setItem(LS_NAME, name.trim());
            } catch {
              /* sem storage: só não lembra o nome */
            }
            onToken(t);
          } catch (err) {
            setFail((err as Error).message);
            setPassword('');
          } finally {
            setBusy(false);
          }
        }}
      >
        <SonaMark size={72} />
        <div className="text-center">
          <h1 className="text-[34px] font-light leading-none tracking-[0.02em] text-hi">sona</h1>
          <p className="mt-2 text-[12.5px] text-subtle">Mesa de som do Lumoverso</p>
        </div>
        {blocked ? (
          <p className="w-full rounded-lg border border-line bg-bg/60 px-3 py-3 text-center text-[12.5px] text-subtle">
            O acesso por outros computadores está desligado. No computador do servidor, abra o Sona e ligue em <b className="text-text">Configurações</b>.
          </p>
        ) : (
          <>
            {remote && (
              <label className="flex w-full flex-col gap-1.5">
                <span className="text-[12px] font-semibold text-subtle">Seu nome</span>
                <input value={name} onChange={(e) => setName(e.target.value)} autoComplete="nickname" className="lm-field h-10 px-3 text-[13px]" placeholder="aparece no servidor" />
              </label>
            )}
            <label className="flex w-full flex-col gap-1.5">
              <span className="text-[12px] font-semibold text-subtle">{remote ? 'Senha de acesso remoto' : 'Senha do app'}</span>
              <input autoFocus type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="current-password" className="lm-field h-10 px-3 text-[13px]" />
            </label>
            {shown && (
              <p role="alert" className="w-full rounded-lg border border-warn/40 bg-warn/10 px-3 py-2 text-[12px] text-warn-light">
                {shown}
              </p>
            )}
            <Btn variant="primary" type="submit" className="h-10 w-full text-[13.5px]" disabled={busy || !password}>
              {busy ? 'Entrando…' : 'Entrar'}
            </Btn>
          </>
        )}
      </form>
    </main>
  );
}
