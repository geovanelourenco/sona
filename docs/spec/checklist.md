# Checklist de implementação

Legenda: ✅ feito e verificado · 🟡 parcial · ⏳ não começado · 🔧 depende de hardware.

## Requisitos funcionais

| ID | Estado | Evidência / o que falta |
|---|---|---|
| FR-01 Listar dispositivos | 🟡 | Bancada 2026-11-27: o Sona listou a Soundcraft no Windows (pares WASAPI); build com ASIO para os 32 canais num dispositivo, pendente de teste.  `sona-server devices`, `GET /api/devices` e vista Áudio do console (principal, entradas e saídas extras, estado, deriva e atraso). Testado só com o ALSA "null". 🔧 Ui24R. |
| FR-02 Captura e saída multicanal | 🟡 | Bancada 2026-11-27: Ui24R pelo ASIO com 32 entradas no mixer e sinal chegando; falta conferir mapeamento canal a canal e as saídas USB.  Mapeamento sem troca provado no motor (`routing.rs::mapeamento_multicanal_sem_troca`). Duplex cpal abre 64×64 no ALSA null. 🔧 Ui24R com sinais distintos. |
| FR-03 Processamento em tempo real | ✅ | `dsp.rs`: ganho, mute com rampa, pan, polaridade, EQ, passa-altas, gate, compressor, limitador, estéreo. `realtime.rs`: zero alocação. |
| FR-04 Buses independentes | ✅ | `routing.rs::buses_sao_independentes` (retorno de PA idêntico amostra a amostra), `envio_pre_fader_ignora_o_fader`. |
| FR-05 Uma fonte em vários destinos | 🟡 | Motor: `uma_fonte_em_dois_destinos`. Vários aparelhos ao mesmo tempo com deriva compensada (ADR 0007, `multidevice.rs`). Falta validar com cabo virtual e OBS reais. |
| FR-06 Controle remoto | ✅ | e2e: dois clientes com o mesmo `rev`; sem clientes o motor segue a 375 blocos/s. |
| FR-07 Monitoração individual | ✅ | Motor: `solo_so_afeta_o_monitor_de_quem_pediu` (programa e outro fone idênticos amostra a amostra). e2e: PFL de canal com fader fechado chega só ao fone 1. Falta escuta no navegador (fase 4). |
| FR-08 Bridge bidirecional | 🟡 | `crates/sona-bridge` (+ tela): envia e recebe ao mesmo tempo, deriva compensada, reconexão, vaga fixa por nome (também quando reconecta antes de a sessão antiga fechar), pacotes autenticados (ADR 0003). e2e pela rede do container: 0 perdas, ~28 ms ida e volta. Escuta local ("ouvir o que chega") com e2e: nível igual ao recebido, volume sem reconectar. 🔧 Falta LAN real e escuta num fone real no Windows. |
| FR-09 Endpoints do sistema | 🟡 | Sem cabo virtual (ADR 0004): plugin "Sona" do OBS (testado com o libobs 30 real) e captura do áudio do sistema no bridge. Não aparece como microfone para outros programas. 🔧 Falta testar no Windows (instalador, OBS, loopback). |
| Perfil Ui24R (numeração da mesa) | 🟡 | `audio/ui24r.rs`, `apply_desk_layout`: mapa USB do manual V1.0 (p. 101), canais na numeração da mesa, migração de instalações antigas com testes. 🔧 Confirmar o mapa na mesa real. |
| FR-13 Player multitrack | 🟡 | ADR 0009. Testes: decodificação e conversão 44,1→48 kHz (`player::decode`), fila de tempo real, busca e fim (`audio::player_rt`), fluxo completo (`player::toca_soma_e_fone`), canais na mesa (`canais_do_player_seguem_o_modo`). e2e FR-13: envio, canal "Player" com nível, click só no fone, fim sozinho, 0 falhas do disco, faixa por canal. Carga com 7 faixas: ver `docs/evidence`. 🔧 Falta tocar no Windows com a Ui24R; marcadores de partes não feitos. |
| FR-10 Persistência e cenas | ✅ | e2e: reinício restaura mix e cenas; entrada ausente vira aviso visível e o motor roda; estado corrompido não derruba. |
| FR-11 Recuperação de falhas | 🟡 | Entrada ausente vira silêncio; principal que falta ou cai é trocado pelo relógio interno e reaberto a cada ~3 s; secundário que cai é reaberto sozinho; console reconecta sozinho. Falta fade na volta e validação com USB real. 🔧 |
| FR-12 Descoberta e autenticação | 🟡 | Primeira configuração e Configurações pela interface: senha de acesso remoto (sem ela, pedidos da rede são recusados) e senha do app (opcional, neste computador). Troca encerra sessões; bloqueio após 5 erros; recuperação pela linha de comando. e2e com pedidos reais pelo IP da rede (11 verificações). Descoberta por mDNS (`sona.local`, lista no bridge, reencontra quando o IP muda). Falta HTTPS fora da LAN; dois servidores na mesma rede disputariam `sona.local` (o bridge lista os dois pelo nome). |

## Segurança de operação

| Item | Estado |
|---|---|
| Ciclo no grafo recusado | ✅ motor + e2e |
| Retorno de PA nasce desligado, ligar pede confirmação | ✅ |
| Aviso de laço pela mesa | ✅ `aviso_de_laco_pela_mesa` |
| Solo nunca afeta transmissão ou PA | ✅ por construção e por teste |
| PA físico independe do servidor | 🔧 arquitetura garante (servidor só lê USB e devolve retornos); validar na igreja |

## Metas de engenharia (medidas, não afirmadas)

| Meta | Estado |
|---|---|
| 24 mono + 4 estéreo de rede, ≥ 4 buses estéreo | 🟡 bench: 7 % do orçamento a 128 amostras, média. Picos isolados acima do orçamento sem prioridade de tempo real. Ver `docs/evidence`. |
| Latência LAN < 20 ms | 🟡 ~14 ms por sentido sem rede física (fila 12 ms + pacote 2,5 ms + bloco); 🔧 medir na LAN |
| 2 h sem underrun | 🔧 precisa do hardware alvo |

## Próximos passos (ordem proposta)

1. Bancada com a Ui24R em Linux e Windows (ADR 0002) — pede acesso ao hardware.
2. Prioridade de tempo real na thread de áudio e novo bench.
3. Validar os instaladores num Windows real (instalação, firewall, autostart).
4. Grupos DCA depois que o grafo estiver estável.
