# ADR 0005 — Plano de controle: HTTP + WebSocket com JSON Merge Patch

- Estado: aceita.

## Decisão

- O servidor é a fonte da verdade. Clientes mandam `patch` (RFC 7386) num
  caminho JSON Pointer da `MixerConfig`; o servidor valida perfil, compila o
  plano, entrega ao motor e transmite o estado inteiro com `rev` para todos.
- Medidores a 20 Hz e estado do motor a 1 Hz pelo mesmo WebSocket.
- Estado pequeno (dezenas de KB): reenviar inteiro é mais simples e evita
  divergência entre operadores. Rever se passar de ~200 KB.
- Autenticação por token (hash SHA-256 no disco). Perfis: admin (tudo),
  operador (mix, sem fonte/saída/ligar retorno), monitor (só o próprio fone).
- Navegador passa o token na query do WebSocket (o navegador não deixa pôr
  cabeçalho). Padrão `127.0.0.1`; LAN com `--bind`. Nada exposto à internet.
- Cenas guardam o mix. Chamar cena não mexe em fontes, saídas, tipo e
  liga/desliga de buses (roteamento físico é do admin e não muda no susto).
- Persistência em `data/state.json` com escrita atômica (temporário + fsync +
  rename), versão de formato e migração. Arquivo ilegível vira cópia
  `.corrupt-*` e o servidor começa do padrão sem cair.

## Pendente

mDNS (descoberta na LAN) com endereço manual como alternativa: fase 3, junto com
o bridge. Revogação de token pela interface. HTTPS para acesso fora da LAN.
