import { useState } from 'react';
import { cx } from './kit';

export const MIN_PASSWORD = 6;

/** Senha + confirmação. Chama onChange com a senha válida, ou null. */
export function NewPassword({ label, onChange, autoFocus }: { label: string; onChange: (v: string | null) => void; autoFocus?: boolean }) {
  const [a, setA] = useState('');
  const [b, setB] = useState('');
  const short = a.length > 0 && a.length < MIN_PASSWORD;
  const differ = b.length > 0 && a !== b;
  const emit = (x: string, y: string) => onChange(x.length >= MIN_PASSWORD && x === y ? x : null);
  return (
    <div className="flex flex-col gap-2">
      <label className="flex flex-col gap-1">
        <span className="text-[11.5px] font-semibold text-subtle">{label}</span>
        <input
          autoFocus={autoFocus}
          type="password"
          autoComplete="new-password"
          value={a}
          onChange={(e) => {
            setA(e.target.value);
            emit(e.target.value, b);
          }}
          className={cx('lm-field h-9 px-3 text-[13px]', short && 'border-warn')}
          placeholder={`mínimo ${MIN_PASSWORD} caracteres`}
        />
      </label>
      <label className="flex flex-col gap-1">
        <span className="text-[11.5px] font-semibold text-subtle">Repita a senha</span>
        <input
          type="password"
          autoComplete="new-password"
          value={b}
          onChange={(e) => {
            setB(e.target.value);
            emit(a, e.target.value);
          }}
          className={cx('lm-field h-9 px-3 text-[13px]', differ && 'border-warn')}
        />
      </label>
      {(short || differ) && <span className="text-[11.5px] text-warn-light">{short ? `Use pelo menos ${MIN_PASSWORD} caracteres.` : 'As senhas não são iguais.'}</span>}
    </div>
  );
}
