import { useState } from 'react';
import { useMixer, tokenStore } from '../lib/store';
import { BUS_KIND_LABEL, type Role, type SourceMap } from '../lib/types';
import { withSend } from './Strip';
import { Btn, Card, cx } from './kit';

function sourceKey(s: SourceMap): string {
  return s.kind === 'none' ? 'none' : s.kind === 'mono' ? `m:${s.input}` : `s:${s.left}:${s.right}`;
}

/** Matriz de roteamento, fontes, saídas, avisos e acessos. */
export function Routing() {
  const st = useMixer((s) => s.state)!;
  const role = useMixer((s) => s.role);
  const patch = useMixer((s) => s.patch);
  const admin = role === 'admin';
  const ins = st.io.inputs;
  const outs = st.io.outputs;
  const sourceOptions: { key: string; label: string; value: SourceMap }[] = [{ key: 'none', label: '— sem fonte —', value: { kind: 'none' } }];
  ins.forEach((p, i) => sourceOptions.push({ key: `m:${i}`, label: p.name, value: { kind: 'mono', input: i } }));
  for (let i = 0; i + 1 < ins.length; i++) sourceOptions.push({ key: `s:${i}:${i + 1}`, label: `${ins[i].name} + ${ins[i + 1].name} (estéreo)`, value: { kind: 'stereo', left: i, right: i + 1 } });

  return (
    <div className="grid min-h-0 flex-1 grid-cols-1 gap-3 overflow-auto p-4 lg:grid-cols-[minmax(0,1fr)_340px]">
      <Card title="Matriz de envios">
        <div className="overflow-auto">
          <table className="border-separate border-spacing-0 text-[12px]">
            <thead>
              <tr>
                <th className="sticky left-0 bg-panel-2 px-2 py-1 text-left text-[11px] font-bold text-muted">Canal</th>
                <th className="px-2 py-1 text-left text-[11px] font-bold text-muted">Fonte</th>
                {st.mixer.buses.map((b, i) => (
                  <th key={i} className="px-1 py-1 text-[11px] font-bold text-muted">
                    <div className="w-20 truncate" title={b.name}>{b.name}</div>
                    <div className="text-[9.5px] font-semibold text-muted/80">{BUS_KIND_LABEL[b.kind]}</div>
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {st.mixer.channels.map((ch, c) => (
                <tr key={c} className="hover:bg-surface/40">
                  <td className="sticky left-0 bg-panel-2 px-2 py-1 font-semibold text-hi">
                    <span className="mr-1.5 font-mono text-[10px] text-muted">{c + 1}</span>
                    {ch.name}
                  </td>
                  <td className="px-2 py-1">
                    <select
                      aria-label={`Fonte de ${ch.name}`}
                      disabled={!admin}
                      value={sourceKey(ch.source)}
                      onChange={(e) => patch(`/channels/${c}`, { source: sourceOptions.find((o) => o.key === e.target.value)!.value })}
                      className="lm-field h-7 max-w-[180px] px-1.5 text-[11.5px] disabled:opacity-60"
                    >
                      {sourceOptions.map((o) => (
                        <option key={o.key} value={o.key}>{o.label}</option>
                      ))}
                    </select>
                  </td>
                  {st.mixer.buses.map((b, i) => {
                    const s = ch.sends.find((x) => x.bus === i);
                    const on = !!s?.enabled && s.level_db > -90;
                    return (
                      <td key={i} className="px-1 py-1 text-center">
                        <button
                          type="button"
                          aria-pressed={on}
                          aria-label={`${ch.name} para ${b.name}`}
                          disabled={role === 'monitor'}
                          onClick={() => patch(`/channels/${c}`, { sends: withSend(ch, i, on ? { enabled: false } : { enabled: true, level_db: s && s.level_db > -90 ? s.level_db : 0 }) })}
                          className={cx('h-7 w-20 rounded-md border font-mono text-[10.5px] transition', on ? 'border-accent/70 bg-accent/20 text-accent-text' : 'border-line bg-bg text-muted hover:border-line-hi')}
                        >
                          {on ? `${s!.level_db.toFixed(1)}${s!.pre_fader ? ' pré' : ''}` : '·'}
                        </button>
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Card>

      <div className="flex flex-col gap-3">
        <Card title="Saídas dos buses">
          {st.mixer.buses.map((b, i) => (
            <label key={i} className="grid grid-cols-[1fr_150px] items-center gap-2">
              <span className="truncate text-[12px] font-semibold text-text">{b.name}</span>
              <select
                aria-label={`Saída de ${b.name}`}
                disabled={!admin}
                value={b.outputs[0] ? String(b.outputs[0].left) : ''}
                onChange={(e) => patch(`/buses/${i}`, { outputs: e.target.value === '' ? [] : [{ left: Number(e.target.value), right: Number(e.target.value) + 1 }] })}
                className="lm-field h-7 px-1.5 text-[11.5px] disabled:opacity-60"
              >
                <option value="">— nenhuma —</option>
                {outs.map((o, k) =>
                  k + 1 < outs.length ? (
                    <option key={k} value={k}>
                      {o.name} / {outs[k + 1].name}
                    </option>
                  ) : null,
                )}
              </select>
            </label>
          ))}
          {!admin && <p className="text-[11.5px] text-muted">Só o admin muda fontes e saídas.</p>}
        </Card>

        <Card title="Avisos">
          {st.validation.warnings.length === 0 && st.validation.errors.length === 0 ? (
            <p className="text-[12px] text-muted">Nenhum aviso de roteamento.</p>
          ) : (
            [...st.validation.errors, ...st.validation.warnings].map((w, i) => (
              <p key={i} className="rounded-lg border border-warn/40 bg-warn/10 px-2.5 py-1.5 text-[12px] text-warn-light">
                {w.message}
              </p>
            ))
          )}
        </Card>

        {admin && <Access />}
      </div>
    </div>
  );
}

function Access() {
  const [name, setName] = useState('');
  const [role, setRole] = useState<Role>('operator');
  const [made, setMade] = useState<{ name: string; link: string } | null>(null);
  const toast = useMixer((s) => s.toast);
  return (
    <Card title="Acessos">
      <p className="text-[11.5px] text-muted">Crie um acesso para outro operador. O link aparece uma vez só.</p>
      <div className="flex gap-2">
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Nome" aria-label="Nome do acesso" className="lm-field h-8 min-w-0 flex-1 px-2.5 text-[12.5px]" />
        <select value={role} onChange={(e) => setRole(e.target.value as Role)} aria-label="Perfil" className="lm-field h-8 px-1.5 text-[12px]">
          <option value="operator">Operador</option>
          <option value="monitor">Só fone</option>
          <option value="admin">Admin</option>
        </select>
      </div>
      <Btn
        variant="primary"
        disabled={!name.trim()}
        onClick={async () => {
          const r = await fetch('/api/tokens', { method: 'POST', headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${tokenStore.get()}` }, body: JSON.stringify({ name: name.trim(), role }) });
          if (!r.ok) return toast('error', 'Não consegui criar o acesso.');
          const { token } = await r.json();
          setMade({ name: name.trim(), link: `${location.origin}/#token=${token}` });
          setName('');
        }}
      >
        Criar acesso
      </Btn>
      {made && (
        <div className="flex flex-col gap-1 rounded-lg border border-accent/40 bg-accent/10 p-2">
          <span className="text-[11.5px] font-semibold text-accent-text">Link de {made.name}</span>
          <input readOnly value={made.link} onFocus={(e) => e.target.select()} className="lm-field h-7 px-2 font-mono text-[10.5px]" aria-label="Link de acesso" />
        </div>
      )}
    </Card>
  );
}
