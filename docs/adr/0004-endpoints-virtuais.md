# ADR 0004 — Endpoints virtuais (dispositivos de áudio do sistema)

- Estado: aceita para a prova de conceito; driver próprio **fora do escopo** até a fase 5.

## Decisão

Na prova de conceito o bridge usa dispositivos virtuais que já existem:

- Windows: um cabo virtual instalado pelo usuário (escolha e licença dele).
- Linux: `pw-loopback` / null sink do PipeWire.
- macOS: dispositivo de loopback instalado pelo usuário.

O OBS captura um desses dispositivos; o Lumo e o navegador tocam em outro.

## Fora do escopo agora

Driver virtual próprio no Windows (assinatura de kernel, instalação, remoção,
ASIO). ASIO sozinho não cria dispositivos de reprodução do Windows. Nenhum
driver sem assinatura é instalado e nenhuma configuração de segurança do sistema
é alterada sem aprovação explícita.
