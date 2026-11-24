//! FR-03: processamento de canal com sinais determinísticos.
mod common;
use common::*;
use sona_core::*;

fn one_channel(ch: ChannelConfig, input: Vec<f32>) -> Vec<Vec<f32>> {
    let cfg = MixerConfig {
        channels: vec![ChannelConfig { sends: vec![send(0, 0.0)], ..ch }],
        buses: vec![out_bus("PGM", BusKind::Broadcast, 0)],
        ..Default::default()
    };
    let (mut e, _h) = engine(&cfg, layout(1, 2));
    let n = input.len();
    run(&mut e, &[input], 2, n)
}

#[test]
fn fader_e_pan_central_mono() {
    let out = one_channel(ChannelConfig { fader_db: -6.0, ..mono("a", 0) }, sine(1000.0, 0.5, 48_000));
    let expected = 0.5 * 10f32.powf(-6.0 / 20.0) * std::f32::consts::FRAC_1_SQRT_2;
    for ch in &out {
        let a = tone_amp(tail(ch, 0.5), 1000.0);
        assert!((a - expected).abs() < 1e-3, "amp {a} esperado {expected}");
    }
}

#[test]
fn trim_soma_ao_fader() {
    let out = one_channel(ChannelConfig { trim_db: 6.0, fader_db: -6.0, pan: -1.0, ..mono("a", 0) }, sine(1000.0, 0.25, 48_000));
    let a = tone_amp(tail(&out[0], 0.5), 1000.0);
    assert!((db(a) - db(0.25)).abs() < 0.05, "trim +6 e fader -6 devem se anular: {} dB", db(a));
}

#[test]
fn pan_total_esquerda_zera_direita() {
    let out = one_channel(ChannelConfig { pan: -1.0, ..mono("a", 0) }, sine(1000.0, 0.5, 24_000));
    assert!((tone_amp(tail(&out[0], 0.5), 1000.0) - 0.5).abs() < 1e-3);
    assert!(peak(tail(&out[1], 0.5)) < 1e-6);
}

#[test]
fn mute_silencia_com_rampa_sem_degrau() {
    // Liga com o canal aberto e depois muta pelo plano novo.
    let cfg_on = MixerConfig {
        channels: vec![ChannelConfig { sends: vec![send(0, 0.0)], pan: -1.0, ..mono("a", 0) }],
        buses: vec![out_bus("PGM", BusKind::Broadcast, 0)],
        ..Default::default()
    };
    let l = layout(1, 2);
    let (mut e, mut h) = engine(&cfg_on, l.clone());
    let dc = vec![vec![1.0f32; 9_600]];
    let before = run(&mut e, &dc, 2, 4_800);
    assert!((before[0][4_799] - 1.0).abs() < 1e-6, "antes: {}", before[0][4_799]);
    let mut cfg_off = cfg_on.clone();
    cfg_off.channels[0].mute = true;
    h.submit(compile(&cfg_off, &l).unwrap().0).unwrap();
    let after = run(&mut e, &dc, 2, 4_800);
    let max_step = after[0].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
    // Fader e envio fazem rampa juntos (produto de duas rampas de 480 amostras):
    // o degrau máximo é 2/480. Um corte seco daria degrau 1.
    assert!(max_step <= 2.0 / 480.0 + 1e-6, "rampa de 10 ms: degrau máximo {max_step}");
    assert!(peak(tail(&after[0], 0.5)) == 0.0, "mudo tem de ser silêncio total");
}

#[test]
fn polaridade_inverte_sinal() {
    let out = one_channel(ChannelConfig { polarity_invert: true, pan: -1.0, ..mono("a", 0) }, vec![0.5; 4_800]);
    assert!((out[0][4_799] + 0.5).abs() < 1e-6);
}

#[test]
fn eq_peaking_mais_6db_em_1khz() {
    let mut ch = ChannelConfig { pan: -1.0, ..mono("a", 0) };
    ch.eq.bands[1] = EqBand { enabled: true, kind: EqBandKind::Peaking, freq_hz: 1000.0, gain_db: 6.0, q: 1.0 };
    let at_1k = one_channel(ch.clone(), sine(1000.0, 0.1, 48_000));
    let at_100 = one_channel(ch, sine(100.0, 0.1, 48_000));
    let g1k = db(tone_amp(tail(&at_1k[0], 0.5), 1000.0) / 0.1);
    let g100 = db(tone_amp(tail(&at_100[0], 0.5), 100.0) / 0.1);
    assert!((g1k - 6.0).abs() < 0.1, "1 kHz: {g1k} dB");
    assert!(g100.abs() < 0.3, "100 Hz quase intacto: {g100} dB");
}

#[test]
fn passa_altas_corta_grave() {
    let mut ch = ChannelConfig { pan: -1.0, ..mono("a", 0) };
    ch.eq.high_pass_enabled = true;
    ch.eq.high_pass_hz = 100.0;
    let low = one_channel(ch.clone(), sine(30.0, 0.5, 96_000));
    let high = one_channel(ch, sine(1000.0, 0.5, 48_000));
    let g30 = db(tone_amp(tail(&low[0], 0.5), 30.0) / 0.5);
    let g1k = db(tone_amp(tail(&high[0], 0.5), 1000.0) / 0.5);
    assert!(g30 < -18.0, "30 Hz deveria cair mais de 18 dB: {g30}");
    assert!(g1k.abs() < 0.1, "1 kHz intacto: {g1k}");
}

#[test]
fn curva_do_biquad_bate_com_o_sinal() {
    let c = biquad::Coefs::peaking(1000.0, 1.0, 6.0, SR);
    assert!((c.magnitude_db(1000.0, SR) - 6.0).abs() < 0.01);
    assert!(c.magnitude_db(50.0, SR).abs() < 0.2);
}

#[test]
fn gate_fecha_abaixo_do_limiar_e_abre_acima() {
    let mut ch = ChannelConfig { pan: -1.0, ..mono("a", 0) };
    ch.gate = GateConfig { enabled: true, threshold_db: -40.0, range_db: 40.0, attack_ms: 0.5, hold_ms: 10.0, release_ms: 20.0 };
    let quiet = one_channel(ch.clone(), sine(1000.0, 10f32.powf(-60.0 / 20.0), 48_000));
    let loud = one_channel(ch, sine(1000.0, 10f32.powf(-20.0 / 20.0), 48_000));
    let q = db(tone_amp(tail(&quiet[0], 0.5), 1000.0));
    let l = db(tone_amp(tail(&loud[0], 0.5), 1000.0));
    assert!((q - -100.0).abs() < 0.5, "fechado: -60 - 40 = -100 dB, medido {q}");
    assert!((l - -20.0).abs() < 0.05, "aberto: intacto, medido {l}");
}

#[test]
fn compressor_reduz_conforme_a_razao() {
    let mut ch = ChannelConfig { pan: -1.0, ..mono("a", 0) };
    ch.comp =
        CompConfig { enabled: true, threshold_db: -20.0, ratio: 4.0, knee_db: 0.0, attack_ms: 1.0, release_ms: 200.0, makeup_db: 0.0 };
    let out = one_channel(ch.clone(), sine(1000.0, 10f32.powf(-10.0 / 20.0), 96_000));
    let o = db(peak(tail(&out[0], 0.25)));
    // 10 dB acima do limiar com razão 4:1 → sai 2,5 dB acima: -17,5 dBFS.
    assert!((o - -17.5).abs() < 0.5, "esperado -17,5 dBFS, medido {o}");
    let below = one_channel(ch, sine(1000.0, 10f32.powf(-30.0 / 20.0), 48_000));
    assert!((db(peak(tail(&below[0], 0.25))) - -30.0).abs() < 0.05, "abaixo do limiar não comprime");
}

#[test]
fn curva_estatica_do_compressor_com_joelho() {
    let p = dynamics::CompParams::new(-20.0, 4.0, 6.0, 1.0, 100.0, 0.0, SR);
    assert_eq!(p.gain_reduction_db(-40.0), 0.0);
    assert!((p.gain_reduction_db(0.0) - -15.0).abs() < 1e-4);
    let at_t = p.gain_reduction_db(-20.0);
    assert!(at_t < 0.0 && at_t > -1.0, "joelho suaviza no limiar: {at_t}");
}

#[test]
fn limitador_segura_o_teto() {
    let cfg = MixerConfig {
        channels: vec![ChannelConfig { sends: vec![send(0, 0.0)], pan: -1.0, ..mono("a", 0) }],
        buses: vec![BusConfig {
            limiter: LimiterConfig { enabled: true, ceiling_db: -6.0, release_ms: 50.0 },
            ..out_bus("PGM", BusKind::Broadcast, 0)
        }],
        ..Default::default()
    };
    let (mut e, _h) = engine(&cfg, layout(1, 2));
    let out = run(&mut e, &[sine(200.0, 1.0, 48_000)], 2, 48_000);
    let ceiling = 10f32.powf(-6.0 / 20.0);
    assert!(peak(&out[0]) <= ceiling + 1e-6, "pico {} acima do teto {ceiling}", peak(&out[0]));
}

#[test]
fn fonte_estereo_mantem_lados() {
    let cfg = MixerConfig {
        channels: vec![ChannelConfig {
            name: "media".into(),
            source: SourceMap::Stereo { left: 0, right: 1 },
            fader_db: 0.0,
            sends: vec![send(0, 0.0)],
            ..Default::default()
        }],
        buses: vec![out_bus("PGM", BusKind::Broadcast, 0)],
        ..Default::default()
    };
    let (mut e, _h) = engine(&cfg, layout(2, 2));
    let out = run(&mut e, &[sine(500.0, 0.5, 24_000), sine(1500.0, 0.5, 24_000)], 2, 24_000);
    assert!((tone_amp(tail(&out[0], 0.5), 500.0) - 0.5).abs() < 1e-3);
    assert!(tone_amp(tail(&out[0], 0.5), 1500.0) < 1e-4);
    assert!((tone_amp(tail(&out[1], 0.5), 1500.0) - 0.5).abs() < 1e-3);
}
