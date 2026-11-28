//! Monta o bloco do motor a partir de várias fontes de relógio: canais do
//! principal (copiados), secundários e rede (via ponte de deriva). Roda dentro
//! do callback do principal: sem alocação, sem trava.

use super::netslot::NetRt;
use super::player_rt::PlayerRt;
use super::{NET_BASE, PORTS};
use sona_core::drift::{Reader, Writer};
use sona_core::Engine;

pub struct SecIn {
    pub base: usize,
    pub channels: usize,
    pub reader: Reader,
}

pub struct SecOut {
    pub base: usize,
    pub channels: usize,
    pub writer: Writer,
}

pub struct Composite {
    pub engine: Engine,
    ins: Vec<Vec<f32>>,
    outs: Vec<Vec<f32>>,
    pub sec_in: Vec<SecIn>,
    pub sec_out: Vec<SecOut>,
    pub net: Vec<NetRt>,
    /// Player multitrack (portas 112+), se ligado.
    pub player: Option<PlayerRt>,
    max_block: usize,
}

impl Composite {
    pub fn new(engine: Engine, max_block: usize, sec_in: Vec<SecIn>, sec_out: Vec<SecOut>, net: Vec<NetRt>) -> Self {
        Composite {
            engine,
            ins: vec![vec![0.0; max_block]; PORTS],
            outs: vec![vec![0.0; max_block]; PORTS],
            sec_in,
            sec_out,
            net,
            player: None,
            max_block,
        }
    }

    pub fn max_block(&self) -> usize {
        self.max_block
    }

    /// Processa `frames` (≤ max_block). `primary_in`/`primary_out` são os
    /// canais do principal (planar); os demais vêm das pontes.
    pub fn run(&mut self, primary_in: &[&[f32]], primary_out: &mut [&mut [f32]], frames: usize) {
        self.run_at(primary_in, primary_out, frames, None);
    }

    /// Igual a `run`, com o instante dado às pontes (segundos; para testes que
    /// simulam o tempo). `None` = relógio de parede.
    pub fn run_at(&mut self, primary_in: &[&[f32]], primary_out: &mut [&mut [f32]], frames: usize, now_s: Option<f64>) {
        let n = frames.min(self.max_block);
        for (i, buf) in self.ins.iter_mut().enumerate() {
            match primary_in.get(i) {
                Some(src) if src.len() >= n => buf[..n].copy_from_slice(&src[..n]),
                _ => buf[..n].fill(0.0),
            }
        }
        for s in self.sec_in.iter_mut() {
            let mut refs: [&mut [f32]; 32] = Default::default();
            let k = s.channels.min(32);
            for (r, b) in refs.iter_mut().zip(self.ins[s.base..s.base + k].iter_mut()) {
                *r = &mut b[..n];
            }
            match now_s {
                Some(t) => s.reader.pull_planar_at(&mut refs[..k], n, t),
                None => s.reader.pull_planar(&mut refs[..k], n),
            }
        }
        for (i, rt) in self.net.iter_mut().enumerate() {
            let base = NET_BASE + 2 * i;
            let (l, r) = self.ins[base..base + 2].split_at_mut(1);
            let mut refs: [&mut [f32]; 2] = [&mut l[0][..n], &mut r[0][..n]];
            match now_s {
                Some(t) => rt.incoming.pull_planar_at(&mut refs, n, t),
                None => rt.incoming.pull_planar(&mut refs, n),
            }
        }
        if let Some(p) = self.player.as_mut() {
            p.render(&mut self.ins, n);
        }
        {
            let in_refs: [&[f32]; PORTS] = std::array::from_fn(|i| &self.ins[i][..n]);
            let mut it = self.outs.iter_mut();
            let mut out_refs: [&mut [f32]; PORTS] = std::array::from_fn(|_| &mut it.next().unwrap()[..n]);
            self.engine.process(&in_refs, &mut out_refs, n);
        }
        for (i, dst) in primary_out.iter_mut().enumerate() {
            let m = n.min(dst.len());
            dst[..m].copy_from_slice(&self.outs[i][..m]);
        }
        for s in self.sec_out.iter_mut() {
            let refs: [&[f32]; 32] = std::array::from_fn(|c| if c < s.channels { &self.outs[s.base + c][..n] } else { &[][..] });
            match now_s {
                Some(t) => s.writer.push_planar_at(&refs[..s.channels.min(32)], n, t),
                None => s.writer.push_planar(&refs[..s.channels.min(32)], n),
            }
        }
        for (i, rt) in self.net.iter_mut().enumerate() {
            let base = NET_BASE + 2 * i;
            // Ninguém lendo: a fila enche e o resto é descartado (sem bloquear).
            let want = (2 * n).min(rt.outgoing.slots()) / 2 * 2;
            if let Ok(mut chunk) = rt.outgoing.write_chunk_uninit(want) {
                let (a, b) = chunk.as_mut_slices();
                for (k, dst) in a.iter_mut().chain(b.iter_mut()).enumerate() {
                    dst.write(self.outs[base + (k % 2)][k / 2]);
                }
                // SAFETY: todos os elementos do pedaço foram escritos acima.
                unsafe { chunk.commit_all() };
            }
        }
    }
}
