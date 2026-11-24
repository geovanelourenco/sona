import { FloppyDisk, Stack } from '@phosphor-icons/react';
import { useEffect, useRef, useState } from 'react';
import { useMixer } from '../lib/store';
import { Btn, cx } from './kit';

/** Cenas: salvar o mix atual e chamar de volta. Chamar cena não mexe no roteamento físico. */
export function Scenes() {
  const st = useMixer((s) => s.state);
  const send = useMixer((s) => s.send);
  const toast = useMixer((s) => s.toast);
  const [open, setOpen] = useState(false);
  const [name, setName] = useState('');
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: PointerEvent) => !box.current?.contains(e.target as Node) && setOpen(false);
    window.addEventListener('pointerdown', close);
    return () => window.removeEventListener('pointerdown', close);
  }, [open]);
  return (
    <div ref={box} className="relative">
      <Btn variant="secondary" onClick={() => setOpen(!open)} aria-expanded={open}>
        <Stack size={15} weight="bold" /> Cenas
      </Btn>
      {open && (
        <div className="absolute right-0 top-10 z-30 flex w-72 flex-col gap-2 rounded-[14px] border border-line bg-panel p-3 shadow-[var(--shadow-modal)]">
          <form
            className="flex gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              if (!name.trim()) return;
              send({ type: 'scene.save', name: name.trim() });
              toast('info', `Cena "${name.trim()}" salva.`);
              setName('');
            }}
          >
            <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Nome da cena" aria-label="Nome da cena" className="lm-field h-8 min-w-0 flex-1 px-2.5 text-[12.5px]" />
            <Btn variant="primary" type="submit" disabled={!name.trim()}>
              <FloppyDisk size={14} weight="bold" /> Salvar
            </Btn>
          </form>
          <p className="text-[11px] text-muted">Chamar uma cena troca o mix (faders, EQ, dinâmica, envios). Fontes e saídas ficam como estão.</p>
          <ul className="flex max-h-64 flex-col gap-1 overflow-auto">
            {(st?.scenes ?? []).length === 0 && <li className="text-[12px] text-muted">Nenhuma cena salva.</li>}
            {st?.scenes.map((s) => (
              <li key={s} className="flex items-center gap-2">
                <button
                  type="button"
                  className={cx('h-8 flex-1 truncate rounded-lg border border-line bg-panel-2 px-2.5 text-left text-[12.5px] font-semibold text-text hover:border-accent/60')}
                  onClick={() => {
                    send({ type: 'scene.recall', name: s });
                    toast('info', `Cena "${s}" chamada.`);
                    setOpen(false);
                  }}
                >
                  {s}
                </button>
                {(
                  <button type="button" aria-label={`Apagar cena ${s}`} className="h-8 rounded-lg px-2 text-[11.5px] text-muted hover:text-live-light" onClick={() => confirm(`Apagar a cena "${s}"?`) && send({ type: 'scene.delete', name: s })}>
                    Apagar
                  </button>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
