//! Vários aparelhos, cada um no seu relógio: um cabo de entrada (+80 ppm), o
//! motor no relógio do principal e um cabo de saída (-120 ppm). Um tom entra
//! pelo secundário de entrada, passa pela mesa e tem de sair limpo no
//! secundário de saída, por minutos, sem falhas.
use sona_core::*;
use sona_server::audio::composite::{Composite, SecIn, SecOut};
use sona_server::audio::{PORTS, SECONDARY_BASE};
use std::f64::consts::TAU;
use std::sync::atomic::Ordering::Relaxed;

const SR: f64 = 48_000.0;

#[test]
fn tom_atravessa_tres_relogios_sem_falhas() {
    let cfg = MixerConfig {
        channels: vec![ChannelConfig {
            name: "Lumo".into(),
            source: SourceMap::Stereo { left: SECONDARY_BASE, right: SECONDARY_BASE + 1 },
            fader_db: 0.0,
            sends: vec![SendConfig { bus: 0, level_db: 0.0, ..Default::default() }],
            ..Default::default()
        }],
        buses: vec![BusConfig {
            outputs: vec![OutputPair { left: SECONDARY_BASE, right: SECONDARY_BASE + 1 }],
            ..BusConfig::new("Transmissão", BusKind::Broadcast)
        }],
        ..Default::default()
    };
    let layout = Layout::new(SR as f32, 128, PORTS, PORTS);
    let (plan, _) = compile(&cfg, &layout).unwrap();
    let (engine, _h) = Engine::new(layout, plan);
    let (mut in_w, in_r, in_stats) = sona_core::drift::bridge(2, SR, 10.0);
    let (out_w, mut out_r, out_stats) = sona_core::drift::bridge(2, SR, 10.0);
    let mut comp = Composite::new(
        engine,
        128,
        vec![SecIn { base: SECONDARY_BASE, channels: 2, reader: in_r }],
        vec![SecOut { base: SECONDARY_BASE, channels: 2, writer: out_w }],
        vec![],
    );

    let (f_in, f_out) = (SR * (1.0 + 80e-6), SR * (1.0 - 120e-6));
    let (b_in, b_main, b_out) = (441usize, 128usize, 512usize);
    let seconds = 120.0;
    let (mut k_in, mut k_main, mut k_out) = (0u64, 0u64, 0u64);
    let mut phase = 0.0f64;
    let mut inbuf = vec![0.0f32; b_in * 2];
    let (mut ol, mut or) = (vec![0.0f32; b_out], vec![0.0f32; b_out]);
    let mut tail = Vec::new();
    let total_out = (seconds * f_out) as u64 / b_out as u64;
    let tail_from = total_out - (f_out as u64 / b_out as u64);
    while k_out < total_out {
        let t_in = k_in as f64 * b_in as f64 / f_in;
        let t_main = k_main as f64 * b_main as f64 / SR;
        let t_out = k_out as f64 * b_out as f64 / f_out;
        if t_in <= t_main && t_in <= t_out {
            for i in 0..b_in {
                let v = (0.4 * phase.sin()) as f32;
                inbuf[2 * i] = v;
                inbuf[2 * i + 1] = v;
                phase = (phase + TAU * 1000.0 / f_in) % TAU; // 1 kHz real
            }
            in_w.push_interleaved_at(&inbuf, t_in);
            k_in += 1;
        } else if t_main <= t_out {
            comp.run_at(&[], &mut [], b_main, Some(t_main));
            k_main += 1;
        } else {
            {
                let mut outs: [&mut [f32]; 2] = [&mut ol, &mut or];
                out_r.pull_planar_at(&mut outs, b_out, t_out);
            }
            if k_out >= tail_from {
                tail.extend_from_slice(&ol);
            }
            k_out += 1;
        }
    }
    let best = (0..200)
        .map(|k| 1000.0 * (1.0 + (k as f64 - 100.0) * 1e-6))
        .map(|f| {
            let (mut ys, mut yc, mut ss, mut cc, mut sc) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for (n, &y) in tail.iter().enumerate() {
                let (s, c) = (TAU * f * n as f64 / f_out).sin_cos();
                ys += y as f64 * s;
                yc += y as f64 * c;
                ss += s * s;
                cc += c * c;
                sc += s * c;
            }
            let det = ss * cc - sc * sc;
            let (a, b) = ((ys * cc - yc * sc) / det, (yc * ss - ys * sc) / det);
            let mut err = 0.0;
            let mut sig = 0.0;
            for (n, &y) in tail.iter().enumerate() {
                let (s, c) = (TAU * f * n as f64 / f_out).sin_cos();
                let v = a * s + b * c;
                sig += v * v;
                err += (y as f64 - v).powi(2);
            }
            (10.0 * (sig / err).log10(), (a * a + b * b).sqrt())
        })
        .fold((f64::MIN, 0.0), |m, x| if x.0 > m.0 { x } else { m });
    println!(
        "SNR {:.1} dB, amplitude {:.4}; entrada: {} underruns, {} overruns, {:.1} ppm; saída: {} underruns, {} overruns, {:.1} ppm",
        best.0,
        best.1,
        in_stats.underruns.load(Relaxed),
        in_stats.overruns.load(Relaxed),
        in_stats.correction_ppm(),
        out_stats.underruns.load(Relaxed),
        out_stats.overruns.load(Relaxed),
        out_stats.correction_ppm()
    );
    assert_eq!(in_stats.underruns.load(Relaxed) + in_stats.overruns.load(Relaxed), 0, "falha na entrada");
    assert_eq!(out_stats.underruns.load(Relaxed) + out_stats.overruns.load(Relaxed), 0, "falha na saída");
    assert!((in_stats.correction_ppm() - 80.0).abs() < 5.0, "entrada deveria corrigir ~+80 ppm");
    assert!((out_stats.correction_ppm() - 120.0).abs() < 5.0, "saída deveria corrigir ~+120 ppm (consumidor mais lento)");
    assert!((best.1 - 0.4).abs() < 0.01, "amplitude {}", best.1);
    assert!(best.0 > 55.0, "SNR {:.1} dB", best.0);
}
