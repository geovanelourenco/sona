# Sona

Mesa de som em software do **Lumoverso**. Junta o roteamento virtual de áudio
(como o Voicemeeter) com o processamento por canal e o fluxo de operação de uma
mesa digital. Caso principal: mix independente de transmissão numa igreja, com
a mídia dos computadores voltando para a mesa física.

O Sona é independente. A integração com o [Lumo](https://github.com/geovanelourenco/lumo)
é opcional e a identidade visual é a mesma (obsidiana, violeta elétrico, ciano aurora).

> **Estado:** fase 0 concluída e primeiro recorte vertical funcionando com
> dispositivo simulado. **Nada foi validado com a Ui24R.** Ver
> [checklist](docs/spec/checklist.md) e [evidências](docs/evidence/README.md).

## O que já funciona

- Motor de áudio em Rust, em tempo real: trim, polaridade, gate, passa-altas,
  EQ paramétrico de 4 bandas, compressor, fader, pan, mute, envios pré/pós,
  buses independentes, subgrupos, limitador de bus e solo PFL/AFL por operador.
- Grafo validado: ciclo entre buses é recusado; retorno de PA nasce desligado;
  aviso de laço quando um canal da mesa volta para a mesma mesa.
- Servidor com controle HTTP + WebSocket, perfis (admin, operador, só fone),
  cenas, persistência atômica com migração e medidores a 20 Hz.
- Console web: faders, medidores, camadas de envio ("sends on faders"),
  processamento com curva de EQ arrastável, matriz de roteamento, cenas e acessos.
- Dispositivos reais via cpal (ASIO/WASAPI/ALSA/JACK/PipeWire/CoreAudio):
  listagem e caminho duplex compilados; duplex só exercitado no ALSA "null".

## Rodar

Requisitos: Rust 1.80+, Node 22+. No Linux, `libasound2-dev` (ou compile com
`--no-default-features` para usar só o dispositivo simulado).

```bash
cd apps/mixer-web && npm ci && npm run build && cd ../..
cargo run --release -p sona-server          # http://127.0.0.1:7340
```

No primeiro início o servidor mostra no log um link com o código de admin.
Para a rede da igreja: `--bind 0.0.0.0:7340` (só em LAN; não exponha à internet).

Outros comandos:

```bash
cargo run --release -p sona-server -- devices                  # dispositivos, canais e taxas (FR-01)
cargo run --release -p sona-server -- --device "Ui24R"         # dispositivo real (não validado)
cargo run --release -p sona-server -- bench                    # custo do motor por bloco
cargo run --release -p sona-server -- token --name Ana --role operator
```

Desenvolvimento da interface com recarga: `npm run dev` em `apps/mixer-web`
(porta 5174, faz proxy de `/api` para a 7340).

## Testes

```bash
cargo test                                   # DSP, roteamento, regra de tempo real
cd apps/mixer-web && npm test                # curva do fader e do EQ
cargo build --release -p sona-server && node tests/e2e/acceptance.mjs
```

## Estrutura

```text
crates/sona-core/     DSP, grafo e motor. Sem I/O, sem rede, sem UI.
crates/sona-server/   hospedagem do motor, dispositivos, controle, persistência
apps/mixer-web/       console web (React + TypeScript + Tailwind)
tests/e2e/            aceitação contra o binário real
docs/spec/            especificação viva e checklist
docs/adr/             decisões de arquitetura
docs/evidence/        medições e resultados de teste
```

O bridge nativo (`apps/bridge`) e o transporte de rede (`crates/sona-net`)
começam na fase 3.
