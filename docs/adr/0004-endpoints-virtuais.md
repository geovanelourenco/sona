# ADR 0004 — Sem dispositivos virtuais: plugin do OBS e captura do sistema

- Estado: aceita (2026-11-26), substitui a versão que usava cabos virtuais de
  terceiros. Decisão do responsável do produto: instalar só o Sona Bridge.

## Contexto

Criar dispositivos de áudio próprios no Windows exige driver de núcleo
assinado pela Microsoft (conta de parceiro + certificado EV), e embutir um
cabo de terceiros exige licença de redistribuição (VB-Cable) e continua sendo
instalação de driver. Os dois casos reais do produto não precisam disso.

## Decisão

- **Receber no OBS:** plugin "Sona" (`apps/obs-plugin`, DLL), instalado pelo
  instalador do Sona Bridge em `C:\ProgramData\obs-studio\plugins\sona-obs`.
  No OBS: fonte de áudio **"Sona (transmissão)"**. O bridge, com o destino de
  recepção **"OBS neste computador"**, serve o áudio em `127.0.0.1:7360`
  (cabeçalho + f32 intercalado, `apps/obs-plugin/src/feed.rs`) no relógio do
  computador, com a deriva da rede já compensada. O plugin procura as funções
  do libobs em tempo de execução (não liga com o SDK); as estruturas espelham
  o libobs 30 e são conferidas por `tests/obs/layout.c`.
- **Enviar do computador de mídia:** o bridge captura o que toca numa saída do
  sistema ("Áudio do sistema — <alto-falante>"), pelo loopback do WASAPI no
  cpal. Aviso na tela se a mesma saída também recebe (laço).
- **Instalador do bridge** passa a pedir administrador (a pasta de plugins do
  OBS é do sistema). Não instala driver.

## Limites

- O Sona não aparece como microfone para outros programas (Zoom, Teams). Se
  for preciso: driver próprio assinado ou cabo licenciado, numa etapa futura.
- Captura por aplicativo (só o Lumo) fica para depois (API de loopback por
  processo, Windows 11 na prática).
- OBS precisa ser reaberto se estava aberto durante a instalação.

## Evidência

- `tests/obs/run.sh` (CI Linux): libobs 30 real, sem interface, carrega o
  plugin, cria a fonte e recebe 384 000 quadros em 8 s com pico 0,2506 (tom
  de 0,5 do bridge, ida e volta pelo servidor, fader −6 dB).
- Loopback e instalação no Windows: **não testados** (pendente).
