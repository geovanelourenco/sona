import { useState } from 'react';
import { useHealth } from '../lib/health';
import { SonaMark } from './Brand';
import { Btn } from './kit';

const LS_NAME = 'sona.name';

export function Login({ onToken, error }: { onToken: (t: string) => void; error?: string }) {
  const health = useHealth();
  const [useCode, setUseCode] = useState(false);
  const [token, setToken] = useState('');
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
  const passwordMode = !!health?.password_login && !useCode;

  const submit = async () => {
    setFail(null);
    if (!passwordMode) {
      if (token.trim()) onToken(token.trim());
      return;
    }
    setBusy(true);
    try {
      const r = await fetch('/api/login', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ password, name: name.trim() }) });
      const j = await r.json().catch(() => ({}));
      if (!r.ok) {
        setFail(j.error ?? 'Não consegui entrar.');
        setPassword('');
        return;
      }
      try {
        localStorage.setItem(LS_NAME, name.trim());
      } catch {
        /* sem storage: só não lembra o nome */
      }
      onToken(j.token);
    } catch {
      setFail('Servidor fora do ar.');
    } finally {
      setBusy(false);
    }
  };

  const shown = fail ?? error;
  return (
    <main className="grid h-full place-items-center p-4">
      <form
        className="flex w-full max-w-sm flex-col items-center gap-5 rounded-[18px] border border-line bg-panel/80 p-8 shadow-[var(--shadow-modal)]"
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <SonaMark size={72} />
        <div className="text-center">
          <h1 className="text-[34px] font-light leading-none tracking-[0.02em] text-hi">sona</h1>
          <p className="mt-2 text-[12.5px] text-subtle">Mesa de som do Lumoverso</p>
        </div>
        {passwordMode ? (
          <>
            <label className="flex w-full flex-col gap-1.5">
              <span className="text-[12px] font-semibold text-subtle">Seu nome</span>
              <input value={name} onChange={(e) => setName(e.target.value)} autoComplete="nickname" className="lm-field h-10 px-3 text-[13px]" placeholder="aparece para os outros operadores" />
            </label>
            <label className="flex w-full flex-col gap-1.5">
              <span className="text-[12px] font-semibold text-subtle">Senha de acesso</span>
              <input autoFocus type="password" value={password} onChange={(e) => setPassword(e.target.value)} autoComplete="current-password" className="lm-field h-10 px-3 text-[13px]" />
            </label>
          </>
        ) : (
          <label className="flex w-full flex-col gap-1.5">
            <span className="text-[12px] font-semibold text-subtle">Código de acesso</span>
            <input autoFocus value={token} onChange={(e) => setToken(e.target.value)} className="lm-field h-10 px-3 font-mono text-[13px]" placeholder="cole o código ou abra o link recebido" />
          </label>
        )}
        {shown && (
          <p role="alert" className="w-full rounded-lg border border-warn/40 bg-warn/10 px-3 py-2 text-[12px] text-warn-light">
            {shown}
          </p>
        )}
        <Btn variant="primary" type="submit" className="h-10 w-full text-[13.5px]" disabled={busy || (passwordMode ? !password : !token.trim())}>
          {busy ? 'Entrando…' : 'Entrar'}
        </Btn>
        {health?.password_login ? (
          <button type="button" onClick={() => setUseCode(!useCode)} className="text-[11.5px] font-semibold text-accent-text hover:underline">
            {useCode ? 'Entrar com senha' : 'Tenho um código de acesso'}
          </button>
        ) : (
          <p className="text-center text-[11.5px] leading-relaxed text-muted">
            O servidor mostra o código de admin no primeiro início. Para entrar por senha de outros computadores, defina a senha na máquina do servidor: <code className="font-mono text-subtle">sona-server password</code>.
          </p>
        )}
      </form>
    </main>
  );
}
