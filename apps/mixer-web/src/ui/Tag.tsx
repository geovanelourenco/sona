import { tagColor } from '../lib/tags';
import { cx } from './kit';

export function TagChip({ tag, onRemove, small, active, onClick }: { tag: string; onRemove?: () => void; small?: boolean; active?: boolean; onClick?: () => void }) {
  const c = tagColor(tag);
  const El = onClick ? 'button' : 'span';
  return (
    <El
      type={onClick ? 'button' : undefined}
      onClick={onClick}
      aria-pressed={onClick ? active : undefined}
      className={cx('inline-flex max-w-full items-center gap-1 truncate rounded-full border font-semibold', small ? 'px-1.5 py-px text-[9.5px]' : 'px-2 py-0.5 text-[11.5px]', onClick && 'transition hover:brightness-125')}
      style={{ color: c, borderColor: active ? c : `${c}55`, background: active ? `${c}2e` : `${c}14` }}
      title={tag}
    >
      <span className="truncate">{tag}</span>
      {onRemove && (
        <button type="button" aria-label={`Tirar a tag ${tag}`} onClick={onRemove} className="-mr-0.5 leading-none opacity-70 hover:opacity-100">
          ×
        </button>
      )}
    </El>
  );
}
