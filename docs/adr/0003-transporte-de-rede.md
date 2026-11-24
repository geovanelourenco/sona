# ADR 0003 — Transporte de áudio em rede (proposta)

- Estado: **proposta**, decisão na fase 3 com medição.

## Proposta

Produção (LAN): RTP sobre UDP com PCM não comprimido (L24 ou float32), 48 kHz,
pacotes de 1 ms ou 2 ms. Cabeçalho RTP dá número de sequência e carimbo de tempo;
sessão negociada pelo canal de controle (WebSocket autenticado, mesmo token).

Cada stream é um domínio de relógio próprio. O receptor estima a deriva pelo
nível do buffer de jitter e corrige com reamostragem adaptativa. Perda: repete o
último pacote com fade, conta e mostra no console.

Monitoração no navegador e pela internet: WebRTC/Opus, fora do caminho de
produção, com atraso maior aceito.

## A medir antes de decidir

Latência ponta a ponta em LAN cabeada (meta < 20 ms), jitter e perdas em Wi-Fi da
igreja, custo de CPU do bridge num notebook comum, deriva real entre dois
computadores ao longo de 2 h.
