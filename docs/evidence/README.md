# Evidências

Resultados reais, com data e máquina. Nada aqui é estimativa.

## 2026-11-24 — container de desenvolvimento (sem hardware de áudio)

Máquina: Intel Xeon @ 2,30 GHz, 4 vCPU, Linux 6.18, sem prioridade de tempo real.

### Testes automáticos

| Suite | Resultado |
|---|---|
| `cargo test -p sona-core` (dsp, routing, realtime) | 29/29 |
| `npm test` em `apps/mixer-web` | 6/6 |
| `node tests/e2e/acceptance.mjs` (binário release, dispositivo simulado) | 10/10 — ver `acceptance-last-run.json` |

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
roteamento (`rotas.png`).
