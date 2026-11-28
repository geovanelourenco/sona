# ADR 0009 — Player multitrack no servidor

- Estado: aceita (2026-11-28).

## Contexto

O operador toca os multitracks do culto num notebook: junta as faixas numa só
e dá play num reprodutor comum. A reprodução multipista do pendrive da Ui24R
não serve ao Sona sem patch e soundcheck na mesa (manual V1.0, seção 5.6). A
igreja quer tocar as faixas separadas pelo Sona, mixar cada uma e, se quiser,
tratar cada faixa como um canal da mesa.

## Decisão

- O player roda **no servidor**, no relógio do motor: não passa por ponte de
  deriva nem por rede. Os arquivos são enviados pelo navegador (pasta ou
  arquivos) e ficam em `data/multitrack/<música>/`; a biblioteca e o
  repertório ficam em `data/multitrack/library.json`.
- Formatos: WAV, AIFF, FLAC e MP3 (symphonia, só as decodificações usadas).
  Taxa diferente da do motor é convertida com Catmull-Rom (a mesma
  interpolação da ponte de deriva). Mono vira L = R.
- **Thread de disco** decodifica até 24 faixas e escreve quadros numa fila SPSC
  de 1 s (`STRIDE` = 48 amostras por quadro). O callback do motor só lê a fila,
  aplica ganho com rampa por bloco e escreve nas portas do player
  (`audio::player_rt`): sem alocação, sem trava, sem I/O.
- Busca e troca de música esvaziam a fila por um pedido/confirmação atômico
  (`flush_req`/`flush_ack`); sem motor rodando, o disco não espera mais que
  300 ms. Falta de dados antes de o disco entregar o primeiro bloco depois da
  busca é espera, não falha (`ready`).
- **Faixa fixa de portas**: 112–113 soma ("Player"), 114–115 click/guia,
  116 + 2k / 117 + 2k faixa k. Domínio de relógio próprio (2000), para o aviso
  de laço pela mesa não tratar o player como aparelho. Capacidade do motor
  sobe para 96 canais.
- **Saída**: "canal único" (padrão) cria o canal estéreo "Player"; "faixa por
  canal" cria um canal "MT <faixa>" por faixa (mono ou estéreo conforme o
  arquivo). Faixas marcadas como fone (click, guia, contagem; detectadas pelo
  nome) nunca entram na soma: vão para "Click + guia", que só envia aos buses
  de fone. Os canais levam a tag "multitrack" e são criados e retirados pelo
  servidor (`Store::sync_player`); ajustes de um canal que continua ficam.
- No modo faixa por canal, o canal k é a faixa na posição k da música atual: ao
  trocar de música o canal ganha o nome da faixa nova e mantém os ajustes.

## Consequências

- O mix por faixa (volume, mute, solo, fone) fica salvo por música; o
  processamento dos canais "MT" é por posição, não por música.
- Cenas são fotos do mix inteiro: chamar uma cena gravada em outro modo traz
  os canais do player daquele momento até a próxima mudança no player.
- Marcadores de partes (intro, refrão) ainda não existem: a linha do tempo só
  mostra posição e permite pular.
