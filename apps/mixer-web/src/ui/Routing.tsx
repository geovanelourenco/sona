import { useMixer } from '../lib/store';
import { BUS_KIND_LABEL, type SourceMap } from '../lib/types';
import { withSend } from './Strip';
import { Card, cx } from './kit';

function sourceKey(s: SourceMap): string {
  return s.kind === 'none' ? 'none' : s.kind === 'mono' ? `m:${s.input}` : `s:${s.left}:${s.right}`;
}

/** Matriz de roteamento, fontes, saídas, avisos e acessos. */
export function Routing() {
  const st = useMixer((s) => s.state)!;
  const patch = useMixer((s) => s.patch);
  const ins = st.io.inputs;
  const outs = st.io.outputs;
  const sourceOptions: { key: string; label: string; value: SourceMap }[] = [{ key: 'none', label: '— sem fonte —', value: { kind: 'none' } }];
  const present = ins.filter((p) => p.present);
  // Pares L/R primeiro (fontes estéreo), depois cada canal.
  for (const p of present) {
    const r = ins[p.index + 1];
    if (!r?.present) continue;
    if (p.name.endsWith(' L') && r.name.endsWith(' R'))
      sourceOptions.push({ key: `s:${p.index}:${r.index}`, label: `${p.name.replace(/ L$/, '')} (estéreo)`, value: { kind: 'stereo', left: p.index, right: r.index } });
    // Entradas da mesa (USB 1, USB 2…): pares ímpar/par, ex. "USB 33 + USB 34" (Player L/R).
    else if (p.index < 64 && p.index % 2 === 0 && p.domain === r.domain)
      sourceOptions.push({ key: `s:${p.index}:${r.index}`, label: `${p.name} + ${r.name} (estéreo)`, value: { kind: 'stereo', left: p.index, right: r.index } });
  }
  for (const p of present) sourceOptions.push({ key: `m:${p.index}`, label: p.name, value: { kind: 'mono', input: p.index } });
  const known = (s: SourceMap) => sourceOptions.some((o) => o.key === sourceKey(s));

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
                     
                      value={sourceKey(ch.source)}
                      onChange={(e) => patch(`/channels/${c}`, { source: sourceOptions.find((o) => o.key === e.target.value)!.value })}
                      className="lm-field h-7 max-w-[180px] px-1.5 text-[11.5px] disabled:opacity-60"
                    >
                      {!known(ch.source) && <option value={sourceKey(ch.source)}>(entrada ausente)</option>}
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
               
                value={b.outputs[0] ? String(b.outputs[0].left) : ''}
                onChange={(e) => patch(`/buses/${i}`, { outputs: e.target.value === '' ? [] : [{ left: Number(e.target.value), right: Number(e.target.value) + 1 }] })}
                className="lm-field h-7 px-1.5 text-[11.5px] disabled:opacity-60"
              >
                <option value="">— nenhuma —</option>
                {outs
                  .filter((o) => o.present && outs[o.index + 1]?.present && (o.index % 2 === 0 || !o.name.endsWith(' R')))
                  .map((o) => (
                    <option key={o.index} value={o.index}>
                      {o.name.endsWith(' L') && outs[o.index + 1].name.endsWith(' R') ? o.name.replace(/ L$/, '') : `${o.name} / ${outs[o.index + 1].name}`}
                      {o.device === 'Sona Bridge' && !o.connected ? ' (aguardando bridge)' : ''}
                    </option>
                  ))}
              </select>
            </label>
          ))}
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

      </div>
    </div>
  );
}

