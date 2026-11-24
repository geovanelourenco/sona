import { useState } from 'react';
import { SonaMark } from './Brand';
import { Btn } from './kit';

export function Login({ onToken, error }: { onToken: (t: string) => void; error?: string }) {
  const [token, setToken] = useState('');
  return (
    <main className="grid h-full place-items-center p-4">
      <form
        className="flex w-full max-w-sm flex-col items-center gap-5 rounded-[18px] border border-line bg-panel/80 p-8 shadow-[var(--shadow-modal)]"
        onSubmit={(e) => {
          e.preventDefault();
          if (token.trim()) onToken(token.trim());
        }}
      >
        <SonaMark size={72} />
        <div className="text-center">
          <h1 className="text-[34px] font-light leading-none tracking-[0.02em] text-hi">sona</h1>
          <p className="mt-2 text-[12.5px] text-subtle">Mesa de som do Lumoverso</p>
        </div>
        <label className="flex w-full flex-col gap-1.5">
          <span className="text-[12px] font-semibold text-subtle">Código de acesso</span>
          <input autoFocus value={token} onChange={(e) => setToken(e.target.value)} className="lm-field h-10 px-3 font-mono text-[13px]" placeholder="cole o código ou abra o link recebido" />
        </label>
        {error && <p className="w-full rounded-lg border border-warn/40 bg-warn/10 px-3 py-2 text-[12px] text-warn-light">{error}</p>}
        <Btn variant="primary" type="submit" className="h-10 w-full text-[13.5px]" disabled={!token.trim()}>
          Entrar
        </Btn>
        <p className="text-center text-[11.5px] leading-relaxed text-muted">
          O servidor mostra o código de admin no primeiro início. Operadores recebem um link pelo admin, em Roteamento → Acessos.
        </p>
      </form>
    </main>
  );
}
