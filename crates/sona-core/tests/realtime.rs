//! Regra de tempo real: `Engine::process` não aloca nem libera memória,
//! inclusive no bloco em que um plano novo é aplicado.
mod common;
use common::*;
use sona_core::*;
use std::alloc::{GlobalAlloc, Layout as AllocLayout, System};
use std::cell::Cell;

struct Counting;
thread_local! { static COUNT: Cell<usize> = const { Cell::new(0) }; static ON: Cell<bool> = const { Cell::new(false) }; }

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: AllocLayout) -> *mut u8 {
        if ON.with(|o| o.get()) {
            COUNT.with(|c| c.set(c.get() + 1));
        }
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: AllocLayout) {
        if ON.with(|o| o.get()) {
            COUNT.with(|c| c.set(c.get() + 1));
        }
        System.dealloc(p, l)
    }
}

#[global_allocator]
static A: Counting = Counting;

fn counted<F: FnOnce()>(f: F) -> usize {
    COUNT.with(|c| c.set(0));
    ON.with(|o| o.set(true));
    f();
    ON.with(|o| o.set(false));
    COUNT.with(|c| c.get())
}

#[test]
fn process_nao_aloca_nem_com_plano_novo() {
    let mut cfg = MixerConfig::default();
    for i in 0..24 {
        let mut ch =
            ChannelConfig { sends: vec![send(0, -3.0), SendConfig { pre_fader: true, ..send(1, -6.0) }], ..mono(&format!("c{i}"), i) };
        ch.eq.high_pass_enabled = true;
        ch.eq.bands[1].gain_db = 3.0;
        ch.gate.enabled = true;
        ch.comp.enabled = true;
        cfg.channels.push(ch);
    }
    cfg.buses.push(BusConfig { limiter: LimiterConfig { enabled: true, ..Default::default() }, ..out_bus("PGM", BusKind::Broadcast, 0) });
    cfg.buses.push(BusConfig { enabled: true, ..out_bus("PA", BusKind::PaReturn, 2) });
    cfg.buses.push(BusConfig {
        monitor: Some(MonitorConfig { follow: Some(0), solo: vec![3], solo_mode: SoloMode::Pfl }),
        ..out_bus("Fone", BusKind::Monitor, 4)
    });
    let l = layout(24, 6);
    let (mut e, mut h) = engine(&cfg, l.clone());

    let inputs: Vec<Vec<f32>> = (0..24).map(|i| sine(100.0 + 50.0 * i as f32, 0.2, BLOCK)).collect();
    let ins: Vec<&[f32]> = inputs.iter().map(|v| v.as_slice()).collect();
    let mut out = vec![vec![0.0f32; BLOCK]; 6];
    let mut outs: Vec<&mut [f32]> = out.iter_mut().map(|v| v.as_mut_slice()).collect();

    let n = counted(|| {
        for _ in 0..200 {
            e.process(&ins, &mut outs, BLOCK);
        }
    });
    assert_eq!(n, 0, "process alocou {n} vezes");

    // Plano novo: o motor troca e devolve o antigo pela fila, sem liberar.
    let mut cfg2 = cfg.clone();
    cfg2.channels[0].fader_db = -10.0;
    h.submit(compile(&cfg2, &l).unwrap().0).unwrap();
    let n = counted(|| e.process(&ins, &mut outs, BLOCK));
    assert_eq!(n, 0, "troca de plano alocou {n} vezes na thread de áudio");
    h.collect();
}

#[test]
fn ponte_de_relogio_nao_aloca() {
    let (mut w, mut r, _stats) = sona_core::drift::bridge(2, SR as f64, 10.0);
    let data = vec![0.1f32; 256];
    let (mut l, mut rr) = (vec![0.0f32; 128], vec![0.0f32; 128]);
    for _ in 0..8 {
        w.push_interleaved(&data);
    }
    let n = counted(|| {
        for _ in 0..1000 {
            w.push_interleaved(&data);
            let mut outs: [&mut [f32]; 2] = [&mut l, &mut rr];
            r.pull_planar(&mut outs, 128);
        }
    });
    assert_eq!(n, 0, "ponte alocou {n} vezes");
}
