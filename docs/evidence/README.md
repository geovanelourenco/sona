# Evidências

Resultados reais, com data e máquina. Nada aqui é estimativa.

## 2026-11-24 — container de desenvolvimento (sem hardware de áudio)

Máquina: Intel Xeon @ 2,30 GHz, 4 vCPU, Linux 6.18, sem prioridade de tempo real.

### Testes automáticos

| Suite | Resultado |
|---|---|
| `cargo test -p sona-core` (dsp, routing, realtime) | 29/29 |
| `npm test` em `apps/mixer-web` | 6/6 |
| `node tests/e2e/acceptance.mjs` (binário release, dispositivo simulado) | 21/21 — ver `acceptance-last-run.json` |
| `cargo test -p sona-server` (senhas e sessões) | 4/4 |

### Custo do motor (`sona-server bench --seconds 20`, release)

24 canais mono + 4 estéreo, 5 buses, HPF + 4 bandas + gate + compressor em todos
os canais, limitador na transmissão, um fone em PFL.

```text
 bloco   média µs     p99 µs     máx µs  orçam. µs     carga
    64       67.8       90.1     4825.5       1333      5.1%
   128      190.4      343.9     2616.7       2667      7.1%
   256      403.9      684.3     2402.8       5333      7.6%
```

Leitura: a média e o p99 ficam muito abaixo do orçamento. O máximo passa do
orçamento a 64 e 128 amostras: são preempções do sistema (a thread não tem
prioridade de tempo real e o container é compartilhado). Não dá para afirmar
"sem underrun" com estes números; precisa repetir no hardware alvo com
prioridade de tempo real.

### Servidor com dispositivo simulado

`/api/health` depois de alguns segundos: carga 1,2–1,5 %, 2 blocos atrasados no
arranque, 0 depois. Um cliente conectado recebe medidores a 20 Hz.

### Duplex cpal no ALSA "null"

`--device "Discard all samples"`: abre 64 entradas e 64 saídas, roda 10 s sem
erro nem pânico. O dispositivo null não segue relógio de tempo real (correu
~110 mil callbacks em 10 s) e por isso `input_starved` e carga não valem como
medição. Prova só que o caminho de abertura, conversão de formato e callback
funciona.

## Pendente de hardware

- Ui24R: canais visíveis por plataforma, duplex, mapeamento, 2 h de estabilidade.
- PA independente do servidor (desligar o servidor no meio do culto de teste).
- Latência e deriva em rede (fase 3).

## Telas (2026-11-24, dispositivo simulado)

`screenshots/`: entrada (`login.png`), mesa (`mesa.png`), processamento do canal
(`detalhe.png`), camada de envios do retorno de PA (`camada-pa.png`) e
roteamento (`rotas.png`), primeira configuração (`primeira-configuracao.png`) e Configurações (`configuracoes.png`).

## Acesso local e remoto (2026-11-24)

O e2e faz pedidos pelo IP de rede do próprio container (`http://192.0.2.2:7340`),
que o servidor vê como outro computador: remoto desligado recusa (403) e não
aceita token local; com senha, entra e o WebSocket funciona; troca de senha e
desligamento derrubam a sessão remota aberta. Telas: `primeira-configuracao.png`
e `configuracoes.png` (tiradas por 127.0.0.1: o Chromium deste container recebe
403 da camada de rede do sandbox no WebSocket para IPs que não são loopback;
Node e curl não têm esse problema). Repetir de outro computador na rede da igreja.

## Vários dispositivos (2026-11-24)

- `cargo test -p sona-core --test drift`: 4/4 (ver ADR 0007).
- `cargo test -p sona-server --test multidevice`: tom por três relógios,
  SNR 63,8 dB, deriva corrigida +83,8 ppm (entrada, real +80) e +120,4 ppm
  (saída, real −120 no consumidor), 0 falhas.
- Servidor real: troca ao vivo para relógio interno + saída no ALSA "null" +
  entrada inexistente. A saída abriu; a entrada ficou "não encontrado" sem
  derrubar nada. As falhas e a deriva medidas no "null" não valem (ele não
  segue relógio). Tela: `screenshots/audio.png`.

## Sona Bridge (2026-11-25)

- `cargo test -p sona-net`: 3/3 (HMAC confere com a RFC 4231, PCM 24 bits ida
  e volta, pacote forjado recusado).
- e2e (bridge pelo IP de rede do container, senha remota, aparelhos simulados a
  +120 ppm): ida e volta 28,1 ms, 0 perdas nos dois sentidos, 0 falhas de áudio
  no servidor, nível 0,554 (tom de 0,5 + sinal de teste da mesa simulada).
  Sem rede física: não vale como medição de LAN.
- A estimativa de deriva no e2e varia (73 a 469 ppm entre execuções) porque os
  relógios do container são de software e perdem blocos sob carga; a precisão
  da estimativa está nos testes determinísticos.
- Telas: `screenshots/bridge-entrada.png`, `screenshots/bridge.png` (a saída
  escolhida na foto é o ALSA "null", por isso as falhas e a deriva mostradas).

## Instaladores Windows (2026-11-25)

Workflow "Instaladores Windows" (runner `windows-latest`, execução 3): build do
Tauri e NSIS com sucesso; `Sona_0.1.0_x64-setup.exe` (3,6 MB) e
`Sona Bridge_0.1.0_x64-setup.exe` (3,3 MB) publicados na Release de
pré-lançamento `instaladores-multi-dispositivo-bridge`. Compilados, **não
instalados nem executados** num Windows real ainda.

## Plugin do OBS e relógio de software (2026-11-26)

Máquina: Intel Xeon @ 2,10 GHz, 4 vCPU (container novo, mais disputado).

- `tests/obs/run.sh`: libobs 30.0.2 real sob Xvfb; plugin carregado, fonte
  "Sona (transmissão)" criada, 384 000 quadros em 8 s, pico 0,2506.
- Defeito achado: o relógio de software (simulado/interno) pulava o tempo
  quando atrasava mais de um bloco e ficava mais lento que o tempo real (a
  deriva medida saturava em 2000 ppm). Agora recupera atrasos de até 200 ms.
- e2e 20/20 em 3 rodadas seguidas; o bridge pode ter 1–2 falhas de fila no
  arranque nesta máquina (pausas de agendamento), absorvidas pela fila
  adaptativa; zero falhas depois de adaptada.

## Descoberta na rede (2026-11-26)

- `cargo test -p sona-net`: anúncio `_sona._tcp` achado e `sona.local`
  respondendo com endereço (mDNS no próprio container).
- e2e: o bridge acha o servidor sem IP (`/api/discover`) e o console lista
  `http://sona.local:7399`. Tela: `screenshots/bridge-descoberta.png`.
- Não testado entre computadores diferentes (o container é um só host).
