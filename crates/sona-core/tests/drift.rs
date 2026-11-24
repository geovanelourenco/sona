//! Ponte entre relógios: deriva real de aparelhos (dezenas a centenas de ppm)
//! e jitter de rede, simulados por minutos de áudio.
use sona_core::drift::bridge;
use std::f64::consts::TAU;
use std::sync::atomic::Ordering::Relaxed;

const SR: f64 = 48_000.0;

struct Sim {
    ppm: f64,
    producer_block: usize,
    consumer_block: usize,
    target_ms: f64,
    /// Atraso aleatório (s) de cada bloco do produtor, como pacotes de rede.
    jitter_s: f64,
    seconds: f64,
}

struct Outcome {
    underruns: u64,
    overruns: u64,
    correction_ppm: f64,
    latency_ms: f64,
    target_ms: f64,
    snr_db: f64,
    tone_amp: f64,
}

/// Gerador pseudoaleatório determinístico (xorshift).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn run(s: Sim) -> Outcome {
    let (mut w, mut r, stats) = bridge(2, SR, s.target_ms);
    let fp = SR * (1.0 + s.ppm * 1e-6);
    let mut rng = Rng(0x9e3779b97f4a7c15);
    let (mut kp, mut kc) = (0u64, 0u64);
    let mut phase = 0.0f64;
    let mut pbuf = vec![0.0f32; s.producer_block * 2];
    let mut out_l = vec![0.0f32; s.consumer_block];
    let mut out_r = vec![0.0f32; s.consumer_block];
    let total_c = (s.seconds * SR) as usize / s.consumer_block;
    let tail_start = total_c.saturating_sub((SR as usize) / s.consumer_block); // último segundo
    let mut tail = Vec::with_capacity(SR as usize + s.consumer_block);
    let mut next_p_jitter = 0.0;
    let avg_start = total_c.saturating_sub((30.0 * SR) as usize / s.consumer_block); // últimos 30 s
    let (mut corr_sum, mut corr_n) = (0.0, 0u64);
    while (kc as usize) < total_c {
        let tp = kp as f64 * s.producer_block as f64 / fp + next_p_jitter;
        let tc = kc as f64 * s.consumer_block as f64 / SR;
        if tp <= tc {
            for i in 0..s.producer_block {
                let v = (0.5 * (phase).sin()) as f32;
                pbuf[2 * i] = v;
                pbuf[2 * i + 1] = -v;
                phase = (phase + TAU * 1000.0 / SR) % TAU; // 1 kHz no relógio do produtor
            }
            w.push_interleaved_at(&pbuf, tp);
            kp += 1;
            next_p_jitter = s.jitter_s * rng.next();
        } else {
            {
                let mut outs: [&mut [f32]; 2] = [&mut out_l, &mut out_r];
                r.pull_planar_at(&mut outs, s.consumer_block, tc);
            }
            if kc as usize >= avg_start {
                corr_sum += stats.correction_ppm();
                corr_n += 1;
            }
            if kc as usize >= tail_start {
                tail.extend_from_slice(&out_l);
            }
            kc += 1;
        }
    }
    // Ajuste por mínimos quadrados de uma senoide. A frequência também é
    // ajustada (±50 ppm em volta de 1 kHz no relógio do produtor): enquanto o
    // controlador converge, a afinação difere alguns ppm (inaudível), e isso
    // não é ruído.
    let fit = |f: f64| {
        let (mut ss, mut sc, mut cc, mut ys, mut yc) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for (n, &y) in tail.iter().enumerate() {
            let (sn, cs) = (TAU * f * n as f64 / SR).sin_cos();
            ss += sn * sn;
            sc += sn * cs;
            cc += cs * cs;
            ys += y as f64 * sn;
            yc += y as f64 * cs;
        }
        let det = ss * cc - sc * sc;
        let a = (ys * cc - yc * sc) / det;
        let b = (yc * ss - ys * sc) / det;
        let (mut sig, mut err) = (0.0, 0.0);
        for (n, &y) in tail.iter().enumerate() {
            let (sn, cs) = (TAU * f * n as f64 / SR).sin_cos();
            let v = a * sn + b * cs;
            sig += v * v;
            err += (y as f64 - v).powi(2);
        }
        (10.0 * (sig / err.max(1e-30)).log10(), (a * a + b * b).sqrt())
    };
    let nominal = 1000.0 * fp / SR;
    let (mut lo, mut hi) = (nominal * (1.0 - 50e-6), nominal * (1.0 + 50e-6));
    for _ in 0..40 {
        let (m1, m2) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
        if fit(m1).0 < fit(m2).0 {
            lo = m1;
        } else {
            hi = m2;
        }
    }
    let (snr_db, tone_amp) = fit((lo + hi) / 2.0);
    Outcome {
        underruns: stats.underruns.load(Relaxed),
        overruns: stats.overruns.load(Relaxed),
        correction_ppm: corr_sum / corr_n.max(1) as f64,
        latency_ms: r.latency_ms(),
        target_ms: r.target_ms(),
        snr_db,
        tone_amp,
    }
}

fn check(name: &str, s: Sim, max_ppm_error: f64) {
    let ppm = s.ppm;
    let o = run(s);
    let target = o.target_ms;
    println!(
        "{name}: correção média (30 s) {:.1} ppm (real {ppm}), latência {:.1} ms (alvo {target}), SNR {:.1} dB, underruns {}, overruns {}",
        o.correction_ppm, o.latency_ms, o.snr_db, o.underruns, o.overruns
    );
    assert_eq!(o.underruns, 0, "{name}: fila esvaziou");
    assert_eq!(o.overruns, 0, "{name}: fila transbordou");
    assert!((o.correction_ppm - ppm).abs() < max_ppm_error, "{name}: estimou {:.1} ppm, real {ppm}", o.correction_ppm);
    assert!((o.latency_ms - target).abs() < target * 0.25, "{name}: latência {:.1} ms longe do alvo {target}", o.latency_ms);
    assert!((o.tone_amp - 0.5).abs() < 0.005, "{name}: amplitude {:.4}", o.tone_amp);
    assert!(o.snr_db > 60.0, "{name}: SNR {:.1} dB", o.snr_db);
}

#[test]
fn produtor_mais_rapido_100ppm() {
    check("+100 ppm", Sim { ppm: 100.0, producer_block: 128, consumer_block: 128, target_ms: 10.0, jitter_s: 0.0, seconds: 180.0 }, 2.0);
}

#[test]
fn produtor_mais_lento_150ppm_blocos_diferentes() {
    check("-150 ppm", Sim { ppm: -150.0, producer_block: 441, consumer_block: 256, target_ms: 15.0, jitter_s: 0.0, seconds: 180.0 }, 2.0);
}

#[test]
fn rede_com_jitter_de_4ms() {
    check(
        "rede +60 ppm, jitter 4 ms",
        Sim { ppm: 60.0, producer_block: 120, consumer_block: 128, target_ms: 20.0, jitter_s: 0.004, seconds: 180.0 },
        3.0,
    );
}

#[test]
fn sem_deriva_fica_em_zero() {
    check("0 ppm", Sim { ppm: 0.0, producer_block: 128, consumer_block: 128, target_ms: 10.0, jitter_s: 0.0, seconds: 180.0 }, 2.0);
}
