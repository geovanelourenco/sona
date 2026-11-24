# Sona — instruções para agentes

Leia `docs/spec/handoff.md` (missão original), `docs/spec/spec.md` e
`docs/spec/checklist.md` antes de mudar código.

- Português do Brasil em interface, documentação e mensagens de commit.
- `Engine::process` não aloca, não bloqueia, não faz I/O. Rode
  `cargo test -p sona-core --test realtime` depois de mexer no motor.
- DSP fica em `crates/sona-core`, sem dependência de transporte, UI ou hardware.
- Toda afirmação de desempenho ou hardware vai para `docs/evidence/` com data,
  máquina e comando. Não afirme o que não mediu; não invente validação na Ui24R.
- Identidade visual: tokens do Lumo (ADR 0006). Vermelho só para "no ar" e clip.
- Mudou um requisito? Atualize spec e checklist no mesmo commit.
- Não instale drivers sem assinatura nem mude segurança do sistema sem aprovação.

Verificação completa:

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
(cd apps/mixer-web && npm run typecheck && npm test && npm run build)
cargo build --release -p sona-server && node tests/e2e/acceptance.mjs
```
