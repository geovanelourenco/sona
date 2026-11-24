# ADR 0007 — Vários dispositivos e compensação de deriva

- Estado: aceita (2026-11-24).

## Contexto

Para a transmissão chegar ao OBS, o servidor precisa ao mesmo tempo da Ui24R
(canais da mesa), de um cabo virtual (OBS na mesma máquina) e de bridges de
rede (OBS e mídia em outras máquinas). Cada um tem o seu relógio; nenhum anda
exatamente a 48 000 Hz.

## Decisão

- Um **dispositivo principal** dita o relógio do motor: a Ui24R, a mesa
  simulada ou um **relógio interno** (servidor sem mesa). Se o principal falta
  ou cai, o relógio interno assume na hora e o servidor tenta reabrir o
  aparelho a cada ~3 s; quando ele volta, os canais são remontados.
- **Secundários** (entradas e saídas extras) e **vagas de rede** passam por
  `sona_core::drift`: fila SPSC + reamostragem cúbica com razão ajustada por um
  PI sobre o nível da fila. O produtor registra o instante de cada bloco para o
  leitor estimar o nível sem o dente de serra de blocos iguais. O alvo da fila
  sobe sozinho até 1,5 bloco de cada lado.
- **Faixas fixas** na lista plana de canais: principal 0–63, secundários 64–95,
  rede 96–111 (8 vagas estéreo). Trocar o principal não embaralha o
  roteamento salvo.
- Configuração em `data/audio.json`, editável no app (vista **Áudio**).
  Aplicar para o áudio por ~1 s: os aparelhos são fechados, o motor é
  recriado com o mesmo mix e tudo reabre.

## Evidência

- `crates/sona-core/tests/drift.rs`: ±100–150 ppm e rede com jitter, 180 s
  simulados: correção a ≤ 0,7 ppm da real, sem falhas, SNR 64–124 dB.
- `crates/sona-server/tests/multidevice.rs`: tom passando por três relógios
  (+80 ppm → motor → −120 ppm), blocos 441/128/512, 120 s: sem falhas, 64 dB.

## Limites

- Reamostragem cúbica (Catmull-Rom): suficiente para correções de centenas de
  ppm; não serve para converter 44,1 ↔ 48 kHz (todos os aparelhos usam a taxa
  configurada).
- Nada validado com aparelhos reais além do ALSA "null" (que não segue relógio).
