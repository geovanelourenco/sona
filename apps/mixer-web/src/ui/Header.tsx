import { GearSix, Headphones, PlugsConnected, SignOut, SlidersHorizontal, TreeStructure } from '@phosphor-icons/react';
import { useMixer } from '../lib/store';
import { SonaLogo } from './Brand';
import { Scenes } from './Scenes';
import { cx } from './kit';

export type View = 'mesa' | 'rotas' | 'audio';

function EngineBadge() {
  const e = useMixer((s) => s.engine);
  const conn = useMixer((s) => s.conn);
  if (!e) return null;
  const ok = conn === 'open' && e.running;
  const load = Math.round(e.load_peak * 1000) / 10;
  return (
    <div className="hidden items-center gap-3 rounded-full border border-line bg-panel/80 px-3 py-1 font-mono text-[11px] text-subtle md:flex" title="Estado do motor de áudio">
      <span className="flex items-center gap-1.5">
        <span className={cx('h-2 w-2 rounded-full', ok ? 'bg-ok shadow-[0_0_8px_#22c55e]' : 'bg-warn')} />
        {ok ? e.device : conn === 'open' ? 'motor parado' : 'sem conexão'}
      </span>
      <span>{(e.sample_rate / 1000).toFixed(0)} kHz · {e.block}</span>
      <span className={cx(load > 70 && 'text-warn-light')}>CPU {load.toFixed(1)}%</span>
      <span className={cx(e.late_blocks > 0 && 'text-warn-light')} title="Blocos que passaram do tempo (equivale a falha no áudio)">
        {e.late_blocks} atrasos
      </span>
      <span title="Navegadores conectados">{e.clients} 👤</span>
    </div>
  );
}

export function Header({ view, setView, onSettings, onLogout }: { view: View; setView: (v: View) => void; onSettings: () => void; onLogout?: () => void }) {
  const st = useMixer((s) => s.state);
  const name = useMixer((s) => s.name);
  const myMonitor = useMixer((s) => s.myMonitor);
  const setMyMonitor = useMixer((s) => s.setMyMonitor);
  const monitors = st?.mixer.buses.map((b, i) => ({ b, i })).filter((x) => x.b.kind === 'monitor') ?? [];

  return (
    <header className="flex h-14 shrink-0 items-center gap-3 border-b border-line/70 bg-panel/60 px-4 backdrop-blur">
      <SonaLogo size={26} />
      <nav className="ml-2 flex rounded-[10px] border border-line bg-bg p-0.5" aria-label="Vistas">
        {(
          [
            ['mesa', 'Mesa', SlidersHorizontal],
            ['rotas', 'Roteamento', TreeStructure],
            ['audio', 'Áudio', PlugsConnected],
          ] as const
        ).map(([v, label, Icon]) => (
          <button
            key={v}
            type="button"
            aria-current={view === v}
            onClick={() => setView(v)}
            className={cx('flex h-7 items-center gap-1.5 rounded-lg px-3 text-[12.5px] font-semibold transition', view === v ? 'bg-surface text-hi' : 'text-subtle hover:text-text')}
          >
            <Icon size={15} weight="bold" />
            {label}
          </button>
        ))}
      </nav>
      <EngineBadge />
      <span className="flex-1" />
      <Scenes />
      <label className="flex items-center gap-1.5 text-[12px] text-subtle" title="Seu fone. PFL só toca aqui.">
        <Headphones size={16} weight="bold" className="text-cyan" />
        <select
          aria-label="Meu fone"
          value={myMonitor ?? ''}
          onChange={(e) => setMyMonitor(e.target.value === '' ? null : Number(e.target.value))}
          className="lm-field h-8 px-2 text-[12px]"
        >
          <option value="">Sem fone</option>
          {monitors.map(({ b, i }) => (
            <option key={i} value={i}>{b.name}</option>
          ))}
        </select>
      </label>
      <span className="hidden rounded-full border border-line px-2.5 py-1 text-[11.5px] font-semibold text-subtle sm:inline">
        {name}
      </span>
      <button type="button" aria-label="Configurações" title="Configurações (acesso e senhas)" onClick={onSettings} className="grid h-8 w-8 place-items-center rounded-lg text-subtle hover:bg-panel-2 hover:text-text">
        <GearSix size={17} weight="bold" />
      </button>
      {onLogout && (
        <button type="button" aria-label="Sair" title="Sair" onClick={onLogout} className="grid h-8 w-8 place-items-center rounded-lg text-subtle hover:bg-panel-2 hover:text-text">
          <SignOut size={16} weight="bold" />
        </button>
      )}
    </header>
  );
}
