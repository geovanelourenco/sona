// Cor fixa por tag (mesma tag, mesma cor), da paleta do Lumoverso.
const PALETTE = ['#a78bfa', '#72d6ff', '#fcd34d', '#4ade80', '#f9a8d4', '#fdba74', '#93c5fd', '#c4b5fd'];

export function tagColor(tag: string): string {
  let h = 0;
  for (const ch of tag.toLowerCase()) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  return PALETTE[h % PALETTE.length];
}

export function normalizeTag(t: string): string {
  return t.trim().replace(/\s+/g, ' ').slice(0, 20);
}
