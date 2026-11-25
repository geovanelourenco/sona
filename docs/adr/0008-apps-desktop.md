# ADR 0008 — Apps desktop em Tauri

- Estado: aceita (2026-11-25). Escolha delegada pelo responsável do produto
  ("o que for mais tranquilo").

## Decisão

Dois apps Tauri 2 (`apps/desktop/sona` e `apps/desktop/bridge`), instaláveis
no Windows com NSIS, por usuário (sem pedir administrador):

- **Sona:** abrir o app liga o servidor de áudio (`sona_server::start`) no
  mesmo processo; a janela mostra o console servido em `127.0.0.1:7340`.
  Fechar a janela esconde na bandeja e o áudio continua; "Sair" na bandeja
  salva e para. "Iniciar com o Windows" abre minimizado no login, então o
  broadcast funciona sem ninguém abrir nada. Uma instância só.
- **Sona Bridge:** o mesmo esquema; tela local em `127.0.0.1:7350`.
- Dados do usuário em `%APPDATA%\app.lumoverso.sona` (e `.bridge`), com
  registro em `sona.log` / `sona-bridge.log`.
- Instalação nova do Sona começa com o relógio interno; a primeira
  configuração pré-seleciona a Ui24R/Soundcraft se ela aparecer.

## Por que Tauri

O servidor e o bridge já são Rust: entram no app como biblioteca, sem Node e
sem processo separado. Instalador pequeno; a interface React é a mesma do
navegador. A janela só carrega a página local (sem IPC do Tauri na página).

## Instaladores

`.github/workflows/desktop.yml` gera os dois instaladores num runner Windows
a cada push e publica numa Release de pré-lançamento `instaladores-<branch>`
(artefatos do Actions esbarraram na cota de armazenamento da conta). Os instaladores não são
assinados: o Windows SmartScreen avisa na primeira execução ("Mais
informações" → "Executar assim mesmo"). Assinatura de código fica para quando
houver certificado.

## Pendente

Validar instalação, firewall (TCP e UDP 7340 na primeira execução) e
autostart num Windows real.
