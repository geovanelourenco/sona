import { Globe, LockSimple } from '@phosphor-icons/react';
import { useState } from 'react';
import { updateAccess } from '../lib/access';
import { SonaMark } from './Brand';
import { NewPassword } from './PasswordField';
import { Btn, Switch } from './kit';

/** Primeira vez que o app abre neste computador: senhas opcionais. */
export function Setup({ onDone }: { onDone: () => void }) {
  const [remoteOn, setRemoteOn] = useState(false);
  const [remote, setRemote] = useState<string | null>(null);
  const [appOn, setAppOn] = useState(false);
  const [app, setApp] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const ready = (!remoteOn || remote) && (!appOn || app);

  return (
    <main className="grid h-full place-items-center overflow-auto p-4">
      <form
        className="flex w-full max-w-md flex-col gap-5 rounded-[18px] border border-line bg-panel/80 p-8 shadow-[var(--shadow-modal)]"
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          setError(null);
          try {
            await updateAccess({ ...(remoteOn ? { remote_password: remote } : {}), ...(appOn ? { app_password: app } : {}), finish_setup: true });
            onDone();
          } catch (err) {
            setError((err as Error).message);
          } finally {
            setBusy(false);
          }
        }}
      >
        <div className="flex items-center gap-4">
          <SonaMark size={56} />
          <div>
            <h1 className="text-[22px] font-light leading-tight text-hi">Bem-vindo ao sona</h1>
            <p className="text-[12.5px] text-subtle">Duas escolhas rápidas. Dá para mudar depois em Configurações.</p>
          </div>
        </div>

        <section className="flex flex-col gap-3 rounded-[14px] border border-line bg-panel-2/60 p-4">
          <div className="flex items-start gap-3">
            <Globe size={20} weight="bold" className="mt-0.5 shrink-0 text-cyan" />
            <div className="flex-1">
              <h2 className="text-[13.5px] font-bold text-hi">Acesso de outros computadores</h2>
              <p className="text-[12px] text-subtle">Notebooks e tablets da rede da igreja abrem o console com esta senha. Sem ela, só este computador acessa.</p>
            </div>
            <Switch label="Permitir acesso remoto" on={remoteOn} onChange={setRemoteOn} />
          </div>
          {remoteOn && <NewPassword label="Senha de acesso remoto" onChange={setRemote} autoFocus />}
        </section>

        <section className="flex flex-col gap-3 rounded-[14px] border border-line bg-panel-2/60 p-4">
          <div className="flex items-start gap-3">
            <LockSimple size={20} weight="bold" className="mt-0.5 shrink-0 text-accent-text" />
            <div className="flex-1">
              <h2 className="text-[13.5px] font-bold text-hi">Senha neste computador</h2>
              <p className="text-[12px] text-subtle">Pede senha para abrir o console aqui também. Útil se outras pessoas usam este computador.</p>
            </div>
            <Switch label="Pedir senha neste computador" on={appOn} onChange={setAppOn} />
          </div>
          {appOn && <NewPassword label="Senha do app" onChange={setApp} />}
        </section>

        {error && <p role="alert" className="rounded-lg border border-warn/40 bg-warn/10 px-3 py-2 text-[12px] text-warn-light">{error}</p>}
        <Btn variant="primary" type="submit" className="h-10 text-[13.5px]" disabled={busy || !ready}>
          {busy ? 'Salvando…' : 'Começar'}
        </Btn>
      </form>
    </main>
  );
}
