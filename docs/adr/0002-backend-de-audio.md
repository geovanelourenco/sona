# ADR 0002 — Backend de áudio: cpal, com plataforma decidida por medição

- Estado: aceita para o backend; plataforma do servidor **em aberto**.

## Decisão

Usar `cpal` 0.18 como camada de dispositivos. Ele cobre ASIO e WASAPI (Windows),
ALSA, JACK e PipeWire (Linux) e CoreAudio (macOS). O servidor também tem um
dispositivo simulado para desenvolvimento e testes.

## Plataforma do servidor

Não está escolhida. O handoff proíbe assumir que Linux ou Windows expõe todos os
canais USB da Ui24R. A escolha depende do teste de bancada abaixo, nas duas
plataformas:

1. `sona-server devices` lista a Ui24R com quantos canais de entrada e saída, em
   48 kHz.
2. `sona-server --device "<nome>"` abre duplex com todos os canais.
3. Sinais distintos (gerador do teste de mapeamento) chegam aos canais certos da
   mesa, sem troca (FR-02).
4. Duas horas sem `late_blocks` nem `input_starved` em 128 amostras.

**Atualização 2026-11-27 (bancada):** no Windows, o driver "Soundcraft MADI-USB
Combo card" apresenta a Ui24R no WASAPI como vários pares estéreo ("Line Out
03/04", "Line Out 05/06"…). Para os canais todos num dispositivo só, o app
Sona no Windows passou a incluir **ASIO** (feature `asio` do `sona-server`,
ligada pelo app desktop). O build baixa o kit da Steinberg (não fica no
repositório) e usa o LLVM do runner. Dispositivos são guardados como
"HOST:Nome" (ex.: "ASIO:Soundcraft…"), porque os pares WASAPI contêm o nome do
dispositivo ASIO. No ASIO o tamanho do bloco é o do driver. Ao escolher a mesa
depois da primeira configuração, o mixer ganha um canal por entrada USB (até
32). **Não validado** com a Ui24R: depende do teste no computador da igreja.

## Limites conhecidos do host atual (`audio/hw.rs`)

- Um único aparelho duplex. Entrada e saída do mesmo aparelho são tratadas como
  o mesmo relógio. Sem compensação de deriva entre aparelhos (fase 3).
- Fila de 8 blocos entre os callbacks de entrada e saída, pré-carga de 1 bloco.
- Formatos aceitos: F32, I32, I24, I16.
- Prioridade de tempo real da thread não é pedida ainda (feature `realtime` do
  cpal); o bench mostra picos de alguns ms por preempção do sistema.
