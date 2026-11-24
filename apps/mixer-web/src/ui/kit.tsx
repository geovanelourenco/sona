import type { ButtonHTMLAttributes, ReactNode } from 'react';

export const cx = (...c: (string | false | null | undefined)[]) => c.filter(Boolean).join(' ');

type Variant = 'primary' | 'outline' | 'secondary' | 'ghost';
const VARIANTS: Record<Variant, string> = {
  primary: 'lm-primary border-0 font-bold',
  outline: 'lm-outline font-bold',
  secondary: 'border border-line bg-panel-2 text-text font-semibold hover:border-line-hi',
  ghost: 'border-0 bg-transparent text-subtle font-semibold hover:bg-panel-2 hover:text-text',
};

export function Btn({ variant = 'secondary', className, ...rest }: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant }) {
  return (
    <button
      type="button"
      {...rest}
      className={cx('inline-flex h-8 shrink-0 items-center justify-center gap-1.5 whitespace-nowrap rounded-lg px-3 text-[12.5px] transition duration-150 ease-out disabled:cursor-not-allowed disabled:opacity-45', VARIANTS[variant], className)}
    />
  );
}

/** Botão de estado da mesa (MUTE, PFL, ON). Aceso = ativo. */
export function Toggle({ on, tone, className, children, ...rest }: ButtonHTMLAttributes<HTMLButtonElement> & { on: boolean; tone: 'warn' | 'cyan' | 'accent' | 'live' | 'ok' }) {
  const lit: Record<typeof tone, string> = {
    warn: 'bg-warn text-deep border-warn shadow-[0_0_14px_rgba(245,158,11,0.45)]',
    cyan: 'bg-cyan text-deep border-cyan shadow-[0_0_14px_rgba(114,214,255,0.45)]',
    accent: 'bg-accent text-white border-accent shadow-[0_0_14px_rgba(124,92,255,0.5)]',
    live: 'bg-live text-white border-live shadow-[0_0_14px_rgba(239,68,68,0.5)]',
    ok: 'bg-ok/20 text-ok-light border-ok/60',
  };
  return (
    <button
      type="button"
      aria-pressed={on}
      {...rest}
      className={cx(
        'h-7 rounded-md border font-mono text-[10.5px] font-medium tracking-wide transition duration-100 disabled:cursor-not-allowed disabled:opacity-35',
        on ? lit[tone] : 'border-line bg-panel-2 text-subtle hover:border-line-hi hover:text-text',
        className,
      )}
    >
      {children}
    </button>
  );
}

export function Chip({ children, className }: { children: ReactNode; className?: string }) {
  return <span className={cx('inline-flex items-center gap-1 rounded-full border border-line px-2 py-0.5 text-[11px] font-semibold text-subtle', className)}>{children}</span>;
}

export function Card({ title, right, children, className }: { title: ReactNode; right?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={cx('flex min-w-0 flex-col gap-2 rounded-[14px] border border-line bg-panel-2/60 p-3', className)}>
      <header className="flex items-center justify-between gap-2">
        <h3 className="text-[11.5px] font-bold uppercase tracking-[0.08em] text-muted">{title}</h3>
        {right}
      </header>
      {children}
    </section>
  );
}

/** Controle numérico com faixa: rótulo, valor em mono e trilho. */
export function Param({ label, value, min, max, step = 0.1, unit = '', onChange, format, disabled, log }: {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  unit?: string;
  onChange: (v: number) => void;
  format?: (v: number) => string;
  disabled?: boolean;
  log?: boolean;
}) {
  const toPos = (v: number) => (log ? Math.log(v / min) / Math.log(max / min) : (v - min) / (max - min));
  const fromPos = (p: number) => (log ? min * Math.pow(max / min, p) : min + p * (max - min));
  const shown = format ? format(value) : `${Math.round(value / step) * step >= 0 ? '' : ''}${Number((Math.round(value / step) * step).toFixed(3))}`;
  return (
    <label className={cx('grid grid-cols-[1fr_auto] items-center gap-x-2 gap-y-1', disabled && 'opacity-45')}>
      <span className="text-[11.5px] font-semibold text-subtle">{label}</span>
      <span className="font-mono text-[11.5px] text-hi">
        {shown}
        {unit && <span className="ml-0.5 text-muted">{unit}</span>}
      </span>
      <input
        type="range"
        aria-label={label}
        className="col-span-2 h-1.5 w-full cursor-pointer accent-accent"
        min={0}
        max={1000}
        disabled={disabled}
        value={Math.round(toPos(value) * 1000)}
        onChange={(e) => {
          const v = fromPos(Number(e.target.value) / 1000);
          onChange(log ? Math.round(v) : Math.round(v / step) * step);
        }}
      />
    </label>
  );
}

export function Switch({ on, onChange, label, disabled }: { on: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!on)}
      className={cx('relative h-5 w-9 shrink-0 rounded-full border transition disabled:opacity-40', on ? 'border-accent bg-accent' : 'border-line-hi bg-bg')}
    >
      <span className={cx('absolute top-0.5 h-3.5 w-3.5 rounded-full bg-white transition-all', on ? 'left-[18px]' : 'left-0.5')} />
    </button>
  );
}
