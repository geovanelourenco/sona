# ADR 0001 — Motor de áudio em Rust

- Estado: aceita (decisão do responsável do produto, 2026-11-24)
- Contexto: o handoff pede avaliar C++/JUCE contra Rust.

## Decisão

Motor, servidor e (na fase 3) bridge em Rust. Interface web em React + TypeScript,
mesma pilha do Lumo.

## Motivos

- Licença: JUCE é AGPLv3 ou licença comercial paga. As bibliotecas Rust usadas
  (cpal, rtrb, axum, tokio, serde) são MIT/Apache-2.0. A distribuição do Sona fica
  livre de royalties e de obrigação copyleft.
- Segurança de memória no callback de tempo real sem custo em tempo de execução.
- O mesmo binário serve sem interface gráfica (serviço), o que o handoff exige.

## Custos aceitos

- Ecossistema de DSP menor que o do JUCE: filtros, dinâmica e suavização foram
  escritos no `sona-core` e cobertos por testes com sinais determinísticos.
- Sem driver virtual pronto em Rust: ver ADR 0004.

## Regra de tempo real (vale para todo o código do motor)

`Engine::process` não aloca, não libera, não bloqueia, não faz I/O. Planos novos
são montados fora da thread de áudio e chegam por fila SPSC sem trava (rtrb); o
plano antigo volta por outra fila para ser liberado no controle. O teste
`crates/sona-core/tests/realtime.rs` conta alocações com um alocador global e
falha se `process` alocar, inclusive no bloco da troca de plano.
