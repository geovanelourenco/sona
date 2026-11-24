# Checklist de implementação

Legenda: ✅ feito e verificado · 🟡 parcial · ⏳ não começado · 🔧 depende de hardware.

## Requisitos funcionais

| ID | Estado | Evidência / o que falta |
|---|---|---|
| FR-01 Listar dispositivos | 🟡 | `sona-server devices` e `GET /api/devices` listam host, canais, taxas e formatos. Testado só com o ALSA "null" do container. 🔧 Ui24R. Falta tela no console. |
| FR-02 Captura e saída multicanal | 🟡 | Mapeamento sem troca provado no motor (`routing.rs::mapeamento_multicanal_sem_troca`). Duplex cpal abre 64×64 no ALSA null. 🔧 Ui24R com sinais distintos. |
| FR-03 Processamento em tempo real | ✅ | `dsp.rs`: ganho, mute com rampa, pan, polaridade, EQ, passa-altas, gate, compressor, limitador, estéreo. `realtime.rs`: zero alocação. |
| FR-04 Buses independentes | ✅ | `routing.rs::buses_sao_independentes` (retorno de PA idêntico amostra a amostra), `envio_pre_fader_ignora_o_fader`. |
| FR-05 Uma fonte em vários destinos | 🟡 | Motor: `uma_fonte_em_dois_destinos`. Falta fonte de rede real (fase 3). |
| FR-06 Controle remoto | ✅ | e2e: dois clientes com o mesmo `rev`; sem clientes o motor segue a 375 blocos/s. |
| FR-07 Monitoração individual | ✅ | Motor: `solo_so_afeta_o_monitor_de_quem_pediu` (programa e outro fone idênticos amostra a amostra). e2e: PFL de canal com fader fechado chega só ao fone 1. Falta escuta no navegador (fase 4). |
| FR-08 Bridge bidirecional | ⏳ | Fase 3. |
| FR-09 Endpoints do sistema | ⏳ | Fase 3 com dispositivos virtuais existentes (ADR 0004). |
| FR-10 Persistência e cenas | ✅ | e2e: reinício restaura mix e cenas; entrada ausente vira aviso visível e o motor roda; estado corrompido não derruba. |
| FR-11 Recuperação de falhas | 🟡 | Motor trata entrada ausente como silêncio. Console reconecta sozinho. Falta: reabrir dispositivo USB que caiu e recuperar com fade; bridge (fase 3). 🔧 |
| FR-12 Descoberta e autenticação | 🟡 | Tokens com hash, perfis admin/operador/monitor, recusa sem token (e2e). Falta mDNS, revogação pela interface, HTTPS fora da LAN. |

## Segurança de operação

| Item | Estado |
|---|---|
| Ciclo no grafo recusado | ✅ motor + e2e |
| Retorno de PA nasce desligado, ligar pede confirmação e perfil admin | ✅ |
| Aviso de laço pela mesa | ✅ `aviso_de_laco_pela_mesa` |
| Solo nunca afeta transmissão ou PA | ✅ por construção e por teste |
| PA físico independe do servidor | 🔧 arquitetura garante (servidor só lê USB e devolve retornos); validar na igreja |

## Metas de engenharia (medidas, não afirmadas)

| Meta | Estado |
|---|---|
| 24 mono + 4 estéreo de rede, ≥ 4 buses estéreo | 🟡 bench: 7 % do orçamento a 128 amostras, média. Picos isolados acima do orçamento sem prioridade de tempo real. Ver `docs/evidence`. |
| Latência LAN < 20 ms | ⏳ fase 3 |
| 2 h sem underrun | 🔧 precisa do hardware alvo |

## Próximos passos (ordem proposta)

1. Bancada com a Ui24R em Linux e Windows (ADR 0002) — pede acesso ao hardware.
2. Prioridade de tempo real na thread de áudio e novo bench.
3. Tela de dispositivos no console (FR-01) e troca de dispositivo sem reiniciar.
4. Fase 3: `crates/sona-net` (RTP/PCM, jitter buffer, deriva) e `apps/bridge`.
5. Grupos DCA depois que o grafo estiver estável.
