# ADR 0005 — Plano de controle: HTTP + WebSocket com JSON Merge Patch

- Estado: aceita.

## Decisão

- O servidor é a fonte da verdade. Clientes mandam `patch` (RFC 7386) num
  caminho JSON Pointer da `MixerConfig`; o servidor valida, compila o
  plano, entrega ao motor e transmite o estado inteiro com `rev` para todos.
- Medidores a 20 Hz e estado do motor a 1 Hz pelo mesmo WebSocket.
- Estado pequeno (dezenas de KB): reenviar inteiro é mais simples e evita
  divergência entre operadores. Rever se passar de ~200 KB.
- Acesso sem perfis (decisão do responsável do produto, 2026-11-24): quem
  entra tem acesso total. Duas senhas, ambas opcionais e definidas pela
  interface (primeira configuração e Configurações), guardadas como Argon2id:
  - **senha do app:** vale para pedidos deste computador (loopback). Sem ela, o
    console abre direto aqui.
  - **senha de acesso remoto:** vale para pedidos de outros computadores. Sem
    ela, o acesso remoto está desligado e esses pedidos são recusados.
- `POST /api/login` troca a senha por um token de sessão (SHA-256 no disco),
  marcado como local ou remoto. Token local não vale vindo da rede.
- Criar ou trocar uma senha encerra as sessões daquele tipo, menos a de quem
  trocou. Desligar o acesso remoto encerra todas as remotas. Tirar a senha do app
  não encerra nada. O WebSocket confere o token a cada 2 s e fecha com `revoked`.
- O servidor escuta em `0.0.0.0` por padrão, para o acesso remoto ser ligado
  sem reiniciar; a recusa é feita por pedido, pela origem.
- Recuperação: `sona-server password --app|--remote --clear` no computador do
  servidor. O arquivo é relido quando muda, então vale sem reiniciar.
- 5 senhas erradas seguidas bloqueiam o IP por 60 s.
- Navegador passa o token na query do WebSocket (o navegador não deixa pôr
  cabeçalho). Nada exposto à internet.
- Cenas guardam o mix. Chamar cena não mexe em fontes, saídas, tipo e
  liga/desliga de buses (roteamento físico não muda no susto).
- Persistência em `data/state.json` com escrita atômica (temporário + fsync +
  rename), versão de formato e migração. Arquivo ilegível vira cópia
  `.corrupt-*` e o servidor começa do padrão sem cair.

## Pendente

mDNS (descoberta na LAN) com endereço manual como alternativa: fase 3, junto com
o bridge. HTTPS para acesso fora da LAN. App de desktop que liga o servidor e
abre o console numa janela própria (hoje: `--open` abre o navegador).
