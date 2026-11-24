# ADR 0006 — Identidade visual do Lumoverso

- Estado: aceita (pedido do responsável do produto).

## Decisão

O Sona usa os tokens do Lumo (`lumo/web/src/index.css`) sem alteração: obsidiana
`#0b0d12`, painéis `#11131a`/`#1a1d26`, violeta `#7c5cff`, ciano `#72d6ff`,
Manrope e JetBrains Mono. Copiados em `apps/mixer-web/src/index.css`; mudança em
um exige mudança no outro.

Regras de cor no contexto de áudio:

- Vermelho: só "NO AR" (transmissão ligada) e LED de clip do medidor.
- Âmbar: MUTE e alertas (retorno de PA ligado, entrada desconectada).
- Ciano: PFL e envios (camada de envios nos faders).
- Violeta: seleção, foco, fader do canal, acento principal.
- Medidor: violeta → ciano → âmbar perto de 0 dBFS.

Marca: anel de vidro com barras de nível, no mesmo material (gradiente violeta →
ciano com brilho) do logotipo do Lumo; palavra "sona" em minúsculas, peso leve.
Arquivo: `apps/mixer-web/public/sona-mark.svg`.
