# ADR 0003 — Transporte de áudio em rede (Sona Bridge)

- Estado: aceita e implementada (2026-11-25). Medição em LAN real pendente.

## Decisão

- **Controle:** o bridge entra com `POST /api/login` (senha de acesso remoto;
  no mesmo computador do servidor, a regra local vale) e guarda só o token.
  Depois abre `ws://servidor/api/bridge?token=…`, manda `bridge.hello` com o
  nome e recebe `bridge.ok` com a vaga, um id de sessão e uma chave de 16 bytes.
  O servidor confere o token a cada segundo: senha remota trocada ou acesso
  remoto desligado encerram a sessão (`bridge.revoked`) e o bridge pede a
  senha de novo. Configuração de áudio aplicada no servidor manda
  `bridge.restart`; o bridge reconecta sozinho na mesma vaga.
- **Áudio:** UDP na mesma porta do HTTP (7340). Pacote com cabeçalho de 36
  bytes (sessão, sequência, instante, quadros) e PCM de 24 bits com sinal,
  120 quadros (2,5 ms) por pacote; assinatura HMAC-SHA256 truncada em 8 bytes
  com a chave da sessão. Pacote com assinatura inválida é descartado e contado.
  Detalhe do formato: `crates/sona-net/src/lib.rs`.
- **Bidirecional:** cada bridge manda um par estéreo (vaga de rede, entrada) e
  recebe um par estéreo (vaga de rede, saída) ao mesmo tempo.
- **Vaga fixa por nome:** `data/bridges.json` guarda nome → vaga; o bridge
  "OBS" volta sempre para a mesma vaga e o roteamento não muda.
- **Endereço aprendido:** o servidor aprende o endereço do bridge do primeiro
  pacote válido (o bridge manda sinal de vida a cada 0,5 s quando não envia
  áudio), o que funciona atrás de NAT simples.
- **Recepção:** quem recebe passa o áudio pela ponte de deriva
  (`sona_core::drift`) com alvo de 12 ms, que cresce 25 % a cada fila vazia até
  80 ms (Wi-Fi ruim). Perda de pacote: silêncio no lugar (mantém o tempo) e
  contagem; pacote atrasado além do seguinte é descartado.
- **Monitoração no navegador e pela internet:** fora deste transporte (fase 4,
  WebRTC/Opus).

## Evidência (container, sem rede física)

`tests/e2e/acceptance.mjs`: bridge pelo IP de rede do container, com senha
remota, aparelhos simulados a +120 ppm: 0 pacotes perdidos, 0 falhas de áudio
no servidor, ida e volta (bridge → servidor → bridge) de ~28 ms, nível
correto; reconexão na mesma vaga após trocar a configuração de áudio; pedido
de senha depois de desligar o acesso remoto.

## Pendente

Medir em LAN cabeada e Wi-Fi da igreja (latência, jitter, perdas), e 2 h de
estabilidade. Criptografia do áudio (hoje só autenticado, não cifrado) se o
uso sair da LAN.
