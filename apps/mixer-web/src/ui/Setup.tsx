import { Globe, LockSimple, Waveform } from '@phosphor-icons/react';
import { useEffect, useState } from 'react';
import { updateAccess } from '../lib/access';
import { tokenStore } from '../lib/store';
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
  // Mesa de som: aparelhos com saída; pré-seleciona a Ui24R/Soundcraft se aparecer.
  const [audioCfg, setAudioCfg] = useState<Record<string, unknown> | null>(null);
  const [devices, setDevices] = useState<{ name: string; host: string }[]>([]);
  const [primary, setPrimary] = useState('');
  useEffect(() => {
    const h = { Authorization: `Bearer ${tokenStore.get()}` };
    Promise.all([fetch('/api/audio', { headers: h }).then((r) => r.json()), fetch('/api/devices', { headers: h }).then((r) => r.json())])
      .then(([a, d]) => {
        type Dev = { name: string; host: string; outputs: { channels: number }[] };
        const list: Dev[] = (d.devices ?? []).filter((x: Dev) => x.outputs?.length);
        setAudioCfg(a.config);
        setDevices(list);
        const ui = list.find((x) => /ui24|soundcraft/i.test(x.name));
        setPrimary(ui?.name ?? a.config.primary);
      })
      .catch(() => {});
  }, []);

  return (
    <main className="grid h-full place-items-center overflow-auto p-4">
      <form
        className="flex w-full max-w-md flex-col gap-5 rounded-[18px] border border-line bg-panel/80 p-8 shadow-[var(--shadow-modal)]"
        onSubmit={async (e) => {
          e.preventDefault();
          setBusy(true);
          setError(null);
          try {
            if (audioCfg && primary && primary !== audioCfg.primary) {
              const r = await fetch('/api/audio', {
                method: 'POST',
                headers: { Authorization: `Bearer ${tokenStore.get()}`, 'Content-Type': 'application/json' },
                body: JSON.stringify({ ...audioCfg, primary }),
              });
              if (!r.ok) throw new Error((await r.json().catch(() => ({}))).error ?? 'Não consegui abrir a mesa de som.');
            }
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
            <p className="text-[12.5px] text-subtle">Três escolhas rápidas. Dá para mudar depois em Áudio e em Configurações.</p>
          </div>
        </div>

        <section className="flex flex-col gap-3 rounded-[14px] border border-line bg-panel-2/60 p-4">
          <div className="flex items-start gap-3">
            <Waveform size={20} weight="bold" className="mt-0.5 shrink-0 text-accent-text" />
            <div className="flex-1">
              <h2 className="text-[13.5px] font-bold text-hi">Mesa de som</h2>
              <p className="text-[12px] text-subtle">O aparelho ligado por USB (ex.: Ui24R). Ele dita o relógio do áudio. Sem mesa agora, deixe o relógio interno e escolha depois em Áudio.</p>
            </div>
          </div>
          <select aria-label="Mesa de som" value={primary} onChange={(e) => setPrimary(e.target.value)} className="lm-field h-9 px-2.5 text-[12.5px]">
            <option value="internal">Relógio interno (sem mesa)</option>
            <option value="sim">Mesa simulada (demonstração)</option>
            {devices.map((d) => (
              <option key={`${d.host}:${d.name}`} value={d.name}>
                {d.name} · {d.host}
              </option>
            ))}
          </select>
        </section>

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
