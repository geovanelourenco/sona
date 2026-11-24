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
- Acesso pela rede por senha, uma por perfil (Argon2id com salt no disco),
  definida na máquina do servidor (`sona-server password`) ou pelo admin no
  console. `POST /api/login` troca a senha por um token de sessão. Trocar ou
  apagar a senha de um perfil derruba as sessões dele (o WebSocket confere o
  token a cada 2 s e fecha com `revoked`). O arquivo é relido quando muda, então
  a linha de comando vale sem reiniciar. 5 erros seguidos bloqueiam o IP por 60 s.
- Sem `--bind`, o servidor escuta em `0.0.0.0` se houver senha e em `127.0.0.1`
  se não houver. Escutar fora do loopback sem senha é recusado na partida.
- Navegador passa o token na query do WebSocket (o navegador não deixa pôr
  cabeçalho). Nada exposto à internet.
- Cenas guardam o mix. Chamar cena não mexe em fontes, saídas, tipo e
  liga/desliga de buses (roteamento físico é do admin e não muda no susto).
- Persistência em `data/state.json` com escrita atômica (temporário + fsync +
  rename), versão de formato e migração. Arquivo ilegível vira cópia
  `.corrupt-*` e o servidor começa do padrão sem cair.

## Pendente

mDNS (descoberta na LAN) com endereço manual como alternativa: fase 3, junto com
o bridge. Revogação de token pela interface. HTTPS para acesso fora da LAN.
