# Sona — especificação viva

Documento de referência do comportamento. Mude junto com o código. Linguagem
controlada, inspirada no ASD-STE100 (sem alegação de conformidade formal).

## 1. Componentes

| Componente | Local | Estado |
|---|---|---|
| Núcleo de áudio (DSP, grafo, motor) | `crates/sona-core` | implementado, testado |
| Servidor de áudio | `crates/sona-server` | implementado com dispositivo simulado; hardware compilado, não validado |
| Console web | `apps/mixer-web` | implementado |
| Bridge nativo | `apps/bridge` | fase 3 |
| Transporte de rede | `crates/sona-net` | fase 3 |

## 2. Caminho do sinal

```text
entrada(s) → trim × polaridade → gate → passa-altas → EQ 4 bandas → compressor
          → pan/balanço → [pré-fader] → fader × mute → [pós-fader]
envio(canal, bus) = (pré ou pós) × nível do envio × (mute ? 0 : 1)
bus = Σ envios (+ envios de outros buses) → fader × mute → limitador → saídas
monitor = solo ativo ? Σ PFL/AFL dos canais em solo : bus acompanhado + envios
```

- Fonte mono: pan com potência constante (−3 dB no centro).
- Fonte estéreo: balanço (0 dB no centro). Filtros por lado; gate e compressor
  com detecção ligada (máximo de L e R).
- Envio pré-fader segue o pan e o mute; não segue o fader.
- Mute do canal zera fader e todos os envios.
- Ganhos (trim, fader, pan, envios, bus) mudam em rampa linear de 10 ms.
- Limitador de bus: sem lookahead; a saída nunca passa do teto.
- Bus desligado (`enabled: false`) processa, mede, mas não escreve em saída.

## 3. Tipos de bus

| Tipo | Uso | Regras |
|---|---|---|
| `broadcast` | transmissão (OBS) | limitador ligado no padrão |
| `pa_return` | retorno USB para a mesa física | nasce desligado; ligar pede confirmação; só admin |
| `recording` | gravação | — |
| `monitor` | fone de um operador | único tipo com solo/PFL e acompanhamento |
| `group` | subgrupo | pode enviar para outros buses |

## 4. Validação do grafo (`sona_core::plan::validate`)

Erros (configuração recusada, motor não muda): entrada, saída, bus ou canal
inexistente; dois envios para o mesmo bus; bus enviando para si mesmo; ciclo
entre buses (inclui acompanhamento de monitor); solo em bus que não é monitor;
mais canais ou buses que a capacidade.

Avisos (configuração aceita): `possible_console_loop` quando um canal vindo de um
aparelho chega, por qualquer caminho, a um `pa_return` ligado que sai para o
mesmo aparelho; `input_disconnected` e `output_disconnected` quando o hardware
atual não tem o canal salvo (a configuração salva não é alterada; o motor usa
silêncio no lugar).

## 5. Tempo real

- Bloco padrão 128 amostras a 48 kHz; aceita qualquer bloco (fatia acima de
  `max_block`).
- Capacidade fixa por motor: 64 canais, 16 buses (memória toda alocada na criação).
- Medidores: atômicos com pico desde a última leitura; leitura a 20 Hz.
- Estatísticas: carga do bloco, pior carga, blocos atrasados, entrada sem dados.

## 6. Protocolo de controle (WebSocket `/api/ws?token=…`)

Cliente → servidor:

| type | campos |
|---|---|
| `patch` | `path` (JSON Pointer), `value` (merge patch) |
| `scene.save` / `scene.recall` / `scene.delete` | `name` |
| `ping` | — |

Servidor → cliente: `hello` (sessão, nome, estado, motor), `state` (estado
inteiro + `rev` + `by`), `meters` (`ch`, `gr`, `gate`, `bus`, `lim`), `engine`,
`error` (`message`, `issues`), `revoked` (sessão encerrada), `pong`.

HTTP:

| Rota | Token | Uso |
|---|---|---|
| `GET /api/health` | não | versão e estado do motor |
| `GET /api/access` | não | `local`, `setup_needed`, `app_password`, `remote_enabled`, `lan_urls` |
| `POST /api/login` | não | `password`, `name` → `token`, `name`. 401 senha errada, 403 remoto desligado, 429 bloqueio |
| `POST /api/access` | sim | `app_password`, `remote_password` (texto ou `null`), `finish_setup` |
| `GET /api/state` | sim | estado |
| `GET /api/devices` | sim | dispositivos de áudio |

Interface estática na raiz (servida também para a rede, para mostrar a tela de
entrada). Regras de acesso: ADR 0005.

## 7. Persistência

`data/state.json` (`format: 1`, `mixer`, `scenes`) e `data/auth.json` (hashes
das sessões, da senha do app e da senha remota; `setup_done`).
Gravação 0,5 s depois da última mudança e no desligamento.

## 8. Requisitos e onde são verificados

Ver `docs/spec/checklist.md`.
