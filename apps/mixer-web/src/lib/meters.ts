// Medidores chegam a 20 Hz. Ficam fora do React: cada medidor lê daqui num
// requestAnimationFrame e mexe direto no estilo, sem re-render.

export const meterData = {
  ch: [] as number[],
  gr: [] as number[],
  gate: [] as number[],
  bus: [] as [number, number][],
  lim: [] as number[],
  at: 0,
};

export function setMeters(m: Omit<typeof meterData, 'at'>) {
  Object.assign(meterData, m, { at: performance.now() });
}

type Tick = (now: number) => void;
const subs = new Set<Tick>();
let raf = 0;

function loop(now: number) {
  for (const s of subs) s(now);
  raf = subs.size ? requestAnimationFrame(loop) : 0;
}

export function onFrame(fn: Tick): () => void {
  subs.add(fn);
  if (!raf) raf = requestAnimationFrame(loop);
  return () => {
    subs.delete(fn);
  };
}
