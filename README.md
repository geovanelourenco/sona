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
- Servidor com controle HTTP + WebSocket, acesso local direto (ou com senha do
  app) e acesso remoto por senha, cenas, persistência atômica com migração e
  medidores a 20 Hz.
- Console web: faders, medidores, camadas de envio ("sends on faders"),
  processamento com curva de EQ arrastável, matriz de roteamento, cenas,
  primeira configuração e Configurações de acesso.
- Dispositivos reais via cpal (ASIO/WASAPI/ALSA/JACK/PipeWire/CoreAudio):
  listagem e caminho duplex compilados; duplex só exercitado no ALSA "null".

## Instalar (Windows)

Baixe os instaladores no GitHub Actions (workflow "Instaladores Windows",
artefato `instaladores-windows`):

- **Sona** (`Sona_…_x64-setup.exe`): no computador ligado à Ui24R. Abrir o app
  liga o servidor; fechar a janela deixa o áudio rodando na bandeja. Marque
  "Iniciar com o Windows" na bandeja para o broadcast funcionar sozinho.
- **Sona Bridge** (`Sona Bridge_…_x64-setup.exe`): no computador do OBS (e
  nos de mídia). Informe o endereço e a senha de acesso remoto do servidor,
  escolha o aparelho que recebe (ex.: "CABLE Input") e, no OBS, capture
  "CABLE Output".

Os instaladores não são assinados: o SmartScreen avisa na primeira vez
("Mais informações" → "Executar assim mesmo"). Na primeira execução, permita
o Sona no firewall (rede privada).

## Rodar a partir do código

Requisitos: Rust 1.80+, Node 22+. No Linux, `libasound2-dev` (ou compile com
`--no-default-features` para usar só o dispositivo simulado).

```bash
cd apps/mixer-web && npm ci && npm run build && cd ../..
cargo run --release -p sona-server          # http://127.0.0.1:7340
```

Ao abrir o console neste computador pela primeira vez, o Sona pede duas
escolhas opcionais (dá para mudar depois em **Configurações**, no ícone de
engrenagem):

- **Acesso de outros computadores:** cria a senha de acesso remoto. Notebooks e
  tablets da rede abrem o endereço mostrado (ex.: `http://192.168.0.10:7340`) e
  entram com ela. Sem essa senha, o servidor recusa qualquer pedido que não venha
  deste computador.
- **Senha neste computador:** o console passa a pedir a senha do app aqui também.
  Sem ela, abre direto.

Não há perfis: quem entra tem acesso total. Trocar uma senha encerra as sessões
abertas com a antiga (menos a de quem trocou); desligar o acesso remoto tira
todos os computadores da rede. Cinco senhas erradas seguidas bloqueiam o IP por
60 s. Só LAN: não há HTTPS ainda; não exponha à internet. No Windows, aceite o
aviso do firewall na primeira vez.

Use `--open` para abrir o console no navegador ao ligar (modo app).

Outros comandos:

```bash
cargo run --release -p sona-server -- devices                  # dispositivos, canais e taxas (FR-01)
cargo run --release -p sona-server -- --device "Ui24R"         # dispositivo real (não validado)
cargo run --release -p sona-server -- bench                    # custo do motor por bloco
cargo run --release -p sona-server -- password --app --clear      # esqueceu a senha do app
cargo run --release -p sona-server -- password --remote --clear   # desliga o acesso remoto
```

Desenvolvimento da interface com recarga: `npm run dev` em `apps/mixer-web`
(porta 5174, faz proxy de `/api` para a 7340).

## Testes

```bash
cargo test                                   # DSP, roteamento, regra de tempo real
cd apps/mixer-web && npm test                # curva do fader e do EQ
cargo build --release -p sona-server -p sona-bridge && node tests/e2e/acceptance.mjs
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

Também: `crates/sona-net` (protocolo de rede), `crates/sona-bridge` (Sona
Bridge) e `apps/desktop/` (apps Tauri; `npx @tauri-apps/cli build` dentro de
`apps/desktop/sona` ou `apps/desktop/bridge`, depois do build da interface).
