/** Marca do Sona: anel de vidro com barras de nível, na linguagem visual do Lumo. */
export function SonaMark({ size = 28 }: { size?: number }) {
  return <img src="/sona-mark.svg" width={size} height={size} alt="" aria-hidden className="drop-shadow-[0_0_12px_rgba(124,92,255,0.45)]" />;
}

export function SonaLogo({ size = 28 }: { size?: number }) {
  return (
    <span className="inline-flex items-center gap-2" aria-label="Sona">
      <SonaMark size={size} />
      <span className="font-light tracking-[0.02em] text-hi/90" style={{ fontSize: size * 0.82, lineHeight: 1 }}>
        sona
      </span>
    </span>
  );
}
