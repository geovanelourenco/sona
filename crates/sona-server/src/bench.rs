//! Mede o custo do motor por bloco, sem dispositivo. Gera evidência para a
//! meta de 24 canais mono + 4 estéreo de rede com DSP completo.

use sona_core::*;
use std::time::Instant;

pub fn heavy_config(mono: usize, stereo: usize) -> (MixerConfig, usize, usize) {
    let mut cfg = MixerConfig::default();
    let buses = [
        BusConfig {
            outputs: vec![OutputPair { left: 0, right: 1 }],
            limiter: LimiterConfig { enabled: true, ..Default::default() },
            ..BusConfig::new("PGM", BusKind::Broadcast)
        },
        BusConfig { enabled: true, outputs: vec![OutputPair { left: 2, right: 3 }], ..BusConfig::new("PA", BusKind::PaReturn) },
        BusConfig { outputs: vec![OutputPair { left: 4, right: 5 }], ..BusConfig::new("REC", BusKind::Recording) },
        BusConfig {
            outputs: vec![OutputPair { left: 6, right: 7 }],
            monitor: Some(MonitorConfig { follow: Some(0), ..Default::default() }),
            ..BusConfig::new("Fone 1", BusKind::Monitor)
        },
        BusConfig {
            outputs: vec![OutputPair { left: 8, right: 9 }],
            monitor: Some(MonitorConfig { follow: Some(0), solo: vec![0, 1], solo_mode: SoloMode::Pfl }),
            ..BusConfig::new("Fone 2", BusKind::Monitor)
        },
    ];
    cfg.buses = buses.to_vec();
    let full = |source| {
        let mut ch = ChannelConfig {
            source,
            fader_db: -3.0,
            sends: vec![
                SendConfig { bus: 0, level_db: -2.0, ..Default::default() },
                SendConfig { bus: 1, level_db: -6.0, pre_fader: true, ..Default::default() },
                SendConfig { bus: 2, level_db: 0.0, ..Default::default() },
            ],
            ..Default::default()
        };
        ch.eq.high_pass_enabled = true;
        for b in ch.eq.bands.iter_mut() {
            b.gain_db = 2.0;
        }
        ch.gate.enabled = true;
        ch.comp.enabled = true;
        ch
    };
    for i in 0..mono {
        cfg.channels.push(full(SourceMap::Mono { input: i }));
    }
    for s in 0..stereo {
        cfg.channels.push(full(SourceMap::Stereo { left: mono + 2 * s, right: mono + 2 * s + 1 }));
    }
    (cfg, mono + 2 * stereo, 10)
}

pub fn run(seconds: f32, sample_rate: u32) {
    println!("Sona bench: 24 mono + 4 estéreo, 5 buses, HPF + 4 bandas + gate + comp em todos os canais.");
    println!("{:>6} {:>10} {:>10} {:>10} {:>10} {:>9}", "bloco", "média µs", "p99 µs", "máx µs", "orçam. µs", "carga");
    for block in [64usize, 128, 256] {
        let (cfg, n_in, n_out) = heavy_config(24, 4);
        let layout = Layout::new(sample_rate as f32, block, n_in, n_out);
        let (plan, _) = compile(&cfg, &layout).expect("config do bench");
        let (mut engine, _h) = Engine::new(layout, plan);
        let ins: Vec<Vec<f32>> = (0..n_in).map(|i| (0..block).map(|k| ((k + i) as f32 * 0.01).sin() * 0.3).collect()).collect();
        let mut outs = vec![vec![0.0f32; block]; n_out];
        let blocks = (seconds * sample_rate as f32 / block as f32) as usize;
        let mut times = Vec::with_capacity(blocks);
        for _ in 0..blocks {
            let in_refs: Vec<&[f32]> = ins.iter().map(|v| v.as_slice()).collect();
            let mut out_refs: Vec<&mut [f32]> = outs.iter_mut().map(|v| v.as_mut_slice()).collect();
            let t0 = Instant::now();
            engine.process(&in_refs, &mut out_refs, block);
            times.push(t0.elapsed().as_secs_f64() * 1e6);
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mean = times.iter().sum::<f64>() / times.len() as f64;
        let p99 = times[(times.len() as f64 * 0.99) as usize];
        let max = *times.last().unwrap();
        let budget = block as f64 / sample_rate as f64 * 1e6;
        println!("{block:>6} {mean:>10.1} {p99:>10.1} {max:>10.1} {budget:>10.0} {:>8.1}%", mean / budget * 100.0);
    }
}
