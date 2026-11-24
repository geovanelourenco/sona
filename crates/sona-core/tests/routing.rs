//! FR-02, FR-04, FR-05, FR-07 e validação do grafo.
mod common;
use common::*;
use sona_core::*;

/// FR-02: oito sinais distintos em oito entradas chegam às oito saídas certas.
#[test]
fn mapeamento_multicanal_sem_troca() {
    let freqs = [300.0, 500.0, 700.0, 1100.0, 1300.0, 1700.0, 1900.0, 2300.0];
    let mut cfg = MixerConfig::default();
    for (i, _) in freqs.iter().enumerate() {
        let bus = i / 2;
        let pan = if i % 2 == 0 { -1.0 } else { 1.0 };
        cfg.channels.push(ChannelConfig { pan, sends: vec![send(bus, 0.0)], ..mono(&format!("in{i}"), i) });
    }
    for b in 0..4 {
        cfg.buses.push(out_bus(&format!("B{b}"), BusKind::Recording, b * 2));
    }
    let (mut e, _h) = engine(&cfg, layout(8, 8));
    let inputs: Vec<Vec<f32>> = freqs.iter().map(|&f| sine(f, 0.25, 24_000)).collect();
    let out = run(&mut e, &inputs, 8, 24_000);
    for (o, wave) in out.iter().enumerate() {
        let t = tail(wave, 0.5);
        for (k, &f) in freqs.iter().enumerate() {
            let a = tone_amp(t, f);
            if k == o {
                assert!((a - 0.25).abs() < 1e-3, "saída {o} deveria ter {f} Hz em 0,25, tem {a}");
            } else {
                assert!(a < 1e-4, "saída {o} vazou {f} Hz ({a})");
            }
        }
    }
}

fn two_mix_config() -> MixerConfig {
    MixerConfig {
        channels: vec![
            ChannelConfig { pan: -1.0, sends: vec![send(0, 0.0), SendConfig { pre_fader: true, ..send(1, -6.0) }], ..mono("voz", 0) },
            ChannelConfig { pan: -1.0, sends: vec![send(0, -6.0), send(1, 0.0)], ..mono("teclado", 1) },
        ],
        buses: vec![
            out_bus("Transmissão", BusKind::Broadcast, 0),
            BusConfig { enabled: true, ..out_bus("Retorno PA", BusKind::PaReturn, 2) },
        ],
        ..Default::default()
    }
}

/// FR-04: mudar um envio para a transmissão não muda o retorno de PA.
#[test]
fn buses_sao_independentes() {
    let l = layout(2, 4);
    let cfg = two_mix_config();
    let inputs = vec![sine(400.0, 0.3, 24_000), sine(900.0, 0.3, 24_000)];
    let (mut e1, _h1) = engine(&cfg, l.clone());
    let base = run(&mut e1, &inputs, 4, 24_000);

    let mut changed = cfg.clone();
    changed.channels[0].sends[0].level_db = -20.0;
    changed.channels[1].sends[0].enabled = false;
    let (mut e2, _h2) = engine(&changed, l);
    let after = run(&mut e2, &inputs, 4, 24_000);

    assert_eq!(base[2], after[2], "retorno de PA tem de ser idêntico amostra a amostra");
    let voz_base = tone_amp(tail(&base[0], 0.5), 400.0);
    let voz_after = tone_amp(tail(&after[0], 0.5), 400.0);
    assert!((db(voz_after) - db(voz_base) - -20.0).abs() < 0.05);
    assert!(tone_amp(tail(&after[0], 0.5), 900.0) < 1e-5);
}

/// Envio pré-fader não segue o fader do canal.
#[test]
fn envio_pre_fader_ignora_o_fader() {
    let l = layout(2, 4);
    let mut cfg = two_mix_config();
    cfg.channels[0].fader_db = -30.0;
    let (mut e, _h) = engine(&cfg, l);
    let out = run(&mut e, &[sine(400.0, 0.3, 24_000), vec![0.0; 24_000]], 4, 24_000);
    let pa = db(tone_amp(tail(&out[2], 0.5), 400.0) / 0.3);
    let pgm = db(tone_amp(tail(&out[0], 0.5), 400.0) / 0.3);
    assert!((pa - -6.0).abs() < 0.05, "PA pré-fader: -6 dB, medido {pa}");
    assert!((pgm - -30.0).abs() < 0.05, "programa pós-fader: -30 dB, medido {pgm}");
}

/// FR-05: uma fonte de rede chega ao mesmo tempo na transmissão e no retorno USB.
#[test]
fn uma_fonte_em_dois_destinos() {
    let cfg = MixerConfig {
        channels: vec![ChannelConfig {
            name: "Lumo".into(),
            source: SourceMap::Stereo { left: 0, right: 1 },
            fader_db: 0.0,
            sends: vec![send(0, 0.0), send(1, -3.0)],
            ..Default::default()
        }],
        buses: vec![out_bus("OBS", BusKind::Broadcast, 0), BusConfig { enabled: true, ..out_bus("USB 23-24", BusKind::PaReturn, 2) }],
        ..Default::default()
    };
    let (mut e, _h) = engine(&cfg, layout(2, 4));
    let out = run(&mut e, &[sine(440.0, 0.4, 24_000), sine(660.0, 0.4, 24_000)], 4, 24_000);
    assert!((tone_amp(tail(&out[0], 0.5), 440.0) - 0.4).abs() < 1e-3);
    assert!((tone_amp(tail(&out[1], 0.5), 660.0) - 0.4).abs() < 1e-3);
    assert!((db(tone_amp(tail(&out[2], 0.5), 440.0) / 0.4) - -3.0).abs() < 0.05);
    assert!((db(tone_amp(tail(&out[3], 0.5), 660.0) / 0.4) - -3.0).abs() < 0.05);
}

/// FR-07: dois operadores, cada um com seu fone. Solo de um não muda o
/// programa nem o fone do outro.
#[test]
fn solo_so_afeta_o_monitor_de_quem_pediu() {
    let mut cfg = two_mix_config();
    let mon = |name: &str, out: usize, solo: Vec<usize>| BusConfig {
        monitor: Some(MonitorConfig { follow: Some(0), solo, solo_mode: SoloMode::Pfl }),
        ..out_bus(name, BusKind::Monitor, out)
    };
    cfg.buses.push(mon("Fone A", 4, vec![]));
    cfg.buses.push(mon("Fone B", 6, vec![]));
    let l = layout(2, 8);
    let inputs = vec![sine(400.0, 0.3, 24_000), sine(900.0, 0.3, 24_000)];
    let (mut e1, _h1) = engine(&cfg, l.clone());
    let base = run(&mut e1, &inputs, 8, 24_000);

    let mut soloed = cfg.clone();
    soloed.buses[2].monitor.as_mut().unwrap().solo = vec![1];
    let (mut e2, _h2) = engine(&soloed, l);
    let after = run(&mut e2, &inputs, 8, 24_000);

    for o in [0, 1, 2, 3, 6, 7] {
        assert_eq!(base[o], after[o], "saída {o} não pode mudar com o solo do fone A");
    }
    // Fone A sem solo acompanha a transmissão; com solo, só o teclado em PFL.
    assert_eq!(base[4], base[0]);
    assert!(tone_amp(tail(&after[4], 0.5), 400.0) < 1e-5, "voz some do fone A durante o solo");
    assert!((tone_amp(tail(&after[4], 0.5), 900.0) - 0.3).abs() < 1e-3, "teclado em PFL, unitário");
}

/// Subgrupo: canal → grupo → transmissão.
#[test]
fn grupo_alimenta_outro_bus() {
    let cfg = MixerConfig {
        channels: vec![ChannelConfig { pan: -1.0, sends: vec![send(1, 0.0)], ..mono("a", 0) }],
        buses: vec![
            out_bus("PGM", BusKind::Broadcast, 0),
            BusConfig {
                fader_db: -6.0,
                bus_sends: vec![BusSendConfig { bus: 0, level_db: 0.0 }],
                ..BusConfig::new("Vozes", BusKind::Group)
            },
        ],
        ..Default::default()
    };
    let (mut e, _h) = engine(&cfg, layout(1, 2));
    let out = run(&mut e, &[sine(1000.0, 0.5, 24_000)], 2, 24_000);
    assert!((db(tone_amp(tail(&out[0], 0.5), 1000.0) / 0.5) - -6.0).abs() < 0.05);
}

#[test]
fn ciclo_entre_buses_e_recusado() {
    let cfg = MixerConfig {
        buses: vec![
            BusConfig { bus_sends: vec![BusSendConfig { bus: 1, level_db: 0.0 }], ..BusConfig::new("A", BusKind::Group) },
            BusConfig { bus_sends: vec![BusSendConfig { bus: 2, level_db: 0.0 }], ..BusConfig::new("B", BusKind::Group) },
            BusConfig { bus_sends: vec![BusSendConfig { bus: 0, level_db: 0.0 }], ..BusConfig::new("C", BusKind::Group) },
        ],
        ..Default::default()
    };
    let err = compile(&cfg, &layout(1, 2)).unwrap_err();
    assert!(err.errors.iter().any(|i| i.code == "cycle"), "{err:?}");
}

#[test]
fn envio_para_si_mesmo_e_recusado() {
    let cfg = MixerConfig {
        buses: vec![BusConfig { bus_sends: vec![BusSendConfig { bus: 0, level_db: 0.0 }], ..BusConfig::new("A", BusKind::Group) }],
        ..Default::default()
    };
    assert!(compile(&cfg, &layout(1, 2)).unwrap_err().errors.iter().any(|i| i.code == "cycle"));
}

#[test]
fn referencias_invalidas_sao_recusadas() {
    let cfg = MixerConfig {
        channels: vec![ChannelConfig { sends: vec![send(5, 0.0), send(0, 0.0), send(0, 0.0)], ..mono("a", 9) }],
        buses: vec![BusConfig { outputs: vec![OutputPair { left: 0, right: 7 }], ..BusConfig::new("A", BusKind::Broadcast) }],
        ..Default::default()
    };
    let codes: Vec<_> = compile(&cfg, &layout(2, 2)).unwrap_err().errors.iter().map(|i| i.code).collect();
    for c in ["input_out_of_range", "bus_out_of_range", "duplicate_send", "output_out_of_range"] {
        assert!(codes.contains(&c), "faltou {c}: {codes:?}");
    }
}

#[test]
fn solo_em_bus_de_programa_e_recusado() {
    let cfg = MixerConfig {
        buses: vec![BusConfig { monitor: Some(MonitorConfig::default()), ..BusConfig::new("PGM", BusKind::Broadcast) }],
        ..Default::default()
    };
    assert!(compile(&cfg, &layout(1, 2)).unwrap_err().errors.iter().any(|i| i.code == "monitor_on_program_bus"));
}

#[test]
fn retorno_de_pa_nasce_desligado() {
    assert!(!BusConfig::new("Retorno", BusKind::PaReturn).enabled);
    let cfg = MixerConfig {
        channels: vec![ChannelConfig { sends: vec![send(0, 0.0)], ..mono("a", 0) }],
        buses: vec![BusConfig { outputs: vec![OutputPair { left: 0, right: 1 }], ..BusConfig::new("Retorno", BusKind::PaReturn) }],
        ..Default::default()
    };
    let (mut e, _h) = engine(&cfg, layout(1, 2));
    let out = run(&mut e, &[vec![0.5; 4_800]], 2, 4_800);
    assert_eq!(peak(&out[0]) + peak(&out[1]), 0.0);
}

#[test]
fn aviso_de_laco_pela_mesa() {
    let mut l = layout(2, 4);
    l.input_domains = vec![1, 2]; // entrada 0 = Ui24R (1), entrada 1 = rede (2)
    l.output_domains = vec![2, 2, 1, 1]; // saídas 2-3 = USB de volta para a Ui24R
    let cfg = two_mix_config();
    let (_, v) = compile(&cfg, &l).unwrap();
    let hits: Vec<_> = v.warnings.iter().filter(|w| w.code == "possible_console_loop").map(|w| w.path.clone()).collect();
    assert_eq!(hits, vec!["/channels/0".to_string()], "só a voz (vem da mesa) volta para a mesa: {:?}", v.warnings);
}

/// FR-11 (parte do motor): entrada que sumiu vale silêncio, sem pânico.
#[test]
fn entrada_ausente_vale_silencio() {
    let cfg = two_mix_config();
    let (mut e, _h) = engine(&cfg, layout(2, 4));
    let out = run(&mut e, &[sine(400.0, 0.3, 4_800)], 4, 4_800);
    assert!(tone_amp(tail(&out[0], 0.5), 900.0) < 1e-6);
}

#[test]
fn bloco_maior_que_o_maximo_e_fatiado() {
    let cfg = two_mix_config();
    let (mut e, _h) = engine(&cfg, layout(2, 4));
    let input = sine(400.0, 0.3, 1000);
    let ins: Vec<&[f32]> = vec![&input, &input];
    let mut out = vec![vec![0.0f32; 1000]; 4];
    let mut outs: Vec<&mut [f32]> = out.iter_mut().map(|v| v.as_mut_slice()).collect();
    e.process(&ins, &mut outs, 1000);
    assert!(peak(&out[0][900..]) > 0.1);
}

#[test]
fn config_ida_e_volta_em_json() {
    let cfg = two_mix_config();
    let json = serde_json::to_string(&cfg).unwrap();
    let back: MixerConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
    let partial: MixerConfig = serde_json::from_str(r#"{"channels":[{"name":"x"}]}"#).unwrap();
    assert_eq!(partial.channels[0].eq, EqConfig::default(), "campos ausentes recebem o padrão");
}
