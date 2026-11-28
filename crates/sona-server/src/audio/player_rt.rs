//! Lado de tempo real do player multitrack. Roda dentro do callback do
//! principal: lê quadros já decodificados de uma fila SPSC (a thread de disco
//! escreve) e escreve nas portas do player. Sem alocação, sem trava.
//!
//! Formato da fila: um quadro = `STRIDE` amostras (L e R de cada uma das
//! `PLAYER_TRACKS` faixas, mesmo as que não existem na música).

use super::{PLAYER_CUE, PLAYER_SUM, PLAYER_TRACKS, PLAYER_TRACK_BASE};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Acquire, Ordering::Relaxed, Ordering::Release};
use std::sync::Arc;

pub const STRIDE: usize = 2 * PLAYER_TRACKS;

/// Estado compartilhado entre controle, disco e tempo real.
pub struct PlayerShared {
    pub playing: AtomicBool,
    /// Quadro da música que o motor toca agora.
    pub position: AtomicU64,
    /// Pedido de esvaziar a fila (busca, troca de música). O tempo real
    /// descarta tudo, põe `position = seek_to` e responde em `flush_ack`.
    pub flush_req: AtomicU64,
    pub flush_ack: AtomicU64,
    pub seek_to: AtomicU64,
    /// Último quadro escrito pelo disco quando chegou ao fim; `u64::MAX` antes.
    pub eof_at: AtomicU64,
    /// A música terminou de tocar (o controle avança o repertório).
    pub ended: AtomicBool,
    /// O disco já pôs dados depois da última busca/troca. Antes disso, falta
    /// de dados é espera normal, não falha.
    pub ready: AtomicBool,
    /// Ganho linear efetivo por faixa (já com mute, solo e volume geral).
    pub gain: [AtomicU32; PLAYER_TRACKS],
    /// Faixa vai para o fone (click, guia) e nunca para a soma.
    pub cue: [AtomicBool; PLAYER_TRACKS],
    /// Cada faixa sai no próprio canal da mesa (senão, soma estéreo).
    pub split: AtomicBool,
    /// Pico por faixa (bits de f32) desde a última leitura.
    pub peak: [AtomicU32; PLAYER_TRACKS],
    /// Blocos em que o disco não entregou a tempo.
    pub underruns: AtomicU64,
}

impl Default for PlayerShared {
    fn default() -> Self {
        PlayerShared {
            playing: AtomicBool::new(false),
            position: AtomicU64::new(0),
            flush_req: AtomicU64::new(0),
            flush_ack: AtomicU64::new(0),
            seek_to: AtomicU64::new(0),
            eof_at: AtomicU64::new(u64::MAX),
            ended: AtomicBool::new(false),
            ready: AtomicBool::new(false),
            gain: std::array::from_fn(|_| AtomicU32::new(0)),
            cue: std::array::from_fn(|_| AtomicBool::new(false)),
            split: AtomicBool::new(false),
            peak: std::array::from_fn(|_| AtomicU32::new(0)),
            underruns: AtomicU64::new(0),
        }
    }
}

impl PlayerShared {
    pub fn set_gain(&self, k: usize, g: f32) {
        if let Some(a) = self.gain.get(k) {
            a.store(g.to_bits(), Relaxed);
        }
    }
    pub fn take_peak(&self, k: usize) -> f32 {
        f32::from_bits(self.peak[k].swap(0, Relaxed))
    }
}

pub struct PlayerRt {
    cons: rtrb::Consumer<f32>,
    sh: Arc<PlayerShared>,
    ack: u64,
    prev: [f32; PLAYER_TRACKS],
}

impl PlayerRt {
    pub fn new(cons: rtrb::Consumer<f32>, sh: Arc<PlayerShared>) -> PlayerRt {
        let ack = sh.flush_req.load(Acquire);
        PlayerRt { cons, sh, ack, prev: [0.0; PLAYER_TRACKS] }
    }

    /// Preenche as portas do player em `ins` (lista plana do motor).
    pub fn render(&mut self, ins: &mut [Vec<f32>], n: usize) {
        for p in ins.iter_mut().skip(PLAYER_SUM).take(PLAYER_TRACK_BASE + 2 * PLAYER_TRACKS - PLAYER_SUM) {
            p[..n].fill(0.0);
        }
        let sh = &*self.sh;
        let req = sh.flush_req.load(Acquire);
        if req != self.ack {
            let k = self.cons.slots();
            if let Ok(c) = self.cons.read_chunk(k) {
                c.commit_all();
            }
            sh.position.store(sh.seek_to.load(Relaxed), Relaxed);
            self.ack = req;
            sh.flush_ack.store(req, Release);
        }
        if !sh.playing.load(Relaxed) {
            return;
        }
        let avail = self.cons.slots() / STRIDE;
        let pos = sh.position.load(Relaxed);
        let eof = sh.eof_at.load(Acquire);
        let m = if avail >= n {
            n
        } else if eof != u64::MAX && pos + avail as u64 >= eof {
            // Fim da música: toca o que sobrou e para.
            sh.playing.store(false, Relaxed);
            sh.ended.store(true, Release);
            avail
        } else {
            if sh.ready.load(Acquire) {
                sh.underruns.fetch_add(1, Relaxed);
            }
            return;
        };
        if m == 0 {
            return;
        }
        let Ok(chunk) = self.cons.read_chunk(m * STRIDE) else { return };
        let (a, b) = chunk.as_slices();
        let at = |i: usize| if i < a.len() { a[i] } else { b[i - a.len()] };
        let split = sh.split.load(Relaxed);
        let inv = 1.0 / m as f32;
        for k in 0..PLAYER_TRACKS {
            let target = f32::from_bits(sh.gain[k].load(Relaxed));
            let start = self.prev[k];
            self.prev[k] = target;
            if start == 0.0 && target == 0.0 {
                continue;
            }
            let cue = sh.cue[k].load(Relaxed);
            let (dl, dr) = if cue {
                (PLAYER_CUE, PLAYER_CUE + 1)
            } else if split {
                (PLAYER_TRACK_BASE + 2 * k, PLAYER_TRACK_BASE + 2 * k + 1)
            } else {
                (PLAYER_SUM, PLAYER_SUM + 1)
            };
            let step = (target - start) * inv;
            let mut peak = 0.0f32;
            for i in 0..m {
                let g = start + step * (i + 1) as f32;
                let l = at(i * STRIDE + 2 * k) * g;
                let r = at(i * STRIDE + 2 * k + 1) * g;
                peak = peak.max(l.abs()).max(r.abs());
                ins[dl][i] += l;
                ins[dr][i] += r;
            }
            if peak.is_finite() {
                sh.peak[k].fetch_max(peak.to_bits(), Relaxed);
            }
        }
        chunk.commit_all();
        sh.position.fetch_add(m as u64, Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::PORTS;

    fn frame(prod: &mut rtrb::Producer<f32>, vals: &[(usize, f32, f32)]) {
        let mut f = [0.0f32; STRIDE];
        for &(k, l, r) in vals {
            f[2 * k] = l;
            f[2 * k + 1] = r;
        }
        for v in f {
            prod.push(v).unwrap();
        }
    }

    #[test]
    fn soma_fone_e_faixa_separada() {
        let sh = Arc::new(PlayerShared::default());
        let (mut prod, cons) = rtrb::RingBuffer::new(STRIDE * 64);
        let mut rt = PlayerRt::new(cons, sh.clone());
        let mut ins = vec![vec![0.0f32; 16]; PORTS];
        sh.cue[0].store(true, Relaxed);
        for k in 0..3 {
            sh.set_gain(k, 1.0);
        }
        // Ganho começa em 0 e sobe em rampa no 1º bloco; o 2º bloco já é cheio.
        sh.playing.store(true, Relaxed);
        for _ in 0..8 {
            frame(&mut prod, &[(0, 0.1, 0.1), (1, 0.2, 0.3), (2, 0.4, 0.4)]);
        }
        rt.render(&mut ins, 4);
        rt.render(&mut ins, 4);
        assert!((ins[PLAYER_CUE][3] - 0.1).abs() < 1e-6, "click no fone");
        assert!((ins[PLAYER_SUM][3] - 0.6).abs() < 1e-6, "baixo + teclas na soma");
        assert!((ins[PLAYER_SUM + 1][3] - 0.7).abs() < 1e-6);
        assert_eq!(sh.position.load(Relaxed), 8);
        // Faixa por canal: cada uma na sua porta, soma vazia.
        sh.split.store(true, Relaxed);
        for _ in 0..4 {
            frame(&mut prod, &[(1, 0.2, 0.3), (2, 0.4, 0.4)]);
        }
        rt.render(&mut ins, 4);
        assert_eq!(ins[PLAYER_SUM][0], 0.0);
        assert!((ins[PLAYER_TRACK_BASE + 2][0] - 0.2).abs() < 1e-6);
        assert!((ins[PLAYER_TRACK_BASE + 5][0] - 0.4).abs() < 1e-6);
    }

    #[test]
    fn busca_esvazia_a_fila_e_fim_para_sozinho() {
        let sh = Arc::new(PlayerShared::default());
        let (mut prod, cons) = rtrb::RingBuffer::new(STRIDE * 64);
        let mut rt = PlayerRt::new(cons, sh.clone());
        let mut ins = vec![vec![0.0f32; 16]; PORTS];
        sh.set_gain(0, 1.0);
        for _ in 0..10 {
            frame(&mut prod, &[(0, 0.5, 0.5)]);
        }
        sh.seek_to.store(1000, Relaxed);
        sh.flush_req.fetch_add(1, Release);
        rt.render(&mut ins, 4);
        assert_eq!(sh.flush_ack.load(Acquire), 1);
        assert_eq!(sh.position.load(Relaxed), 1000);
        assert_eq!(prod.slots(), STRIDE * 64, "fila vazia");
        // Sem dados logo depois da busca: espera, não é falha.
        sh.playing.store(true, Relaxed);
        rt.render(&mut ins, 4);
        assert_eq!(sh.underruns.load(Relaxed), 0);
        // Disco já entregou e mesmo assim faltou: falha.
        sh.ready.store(true, Release);
        rt.render(&mut ins, 4);
        assert_eq!(sh.underruns.load(Relaxed), 1);
        // Últimos 3 quadros da música.
        for _ in 0..3 {
            frame(&mut prod, &[(0, 0.5, 0.5)]);
        }
        sh.eof_at.store(1003, Release);
        rt.render(&mut ins, 4);
        assert!(!sh.playing.load(Relaxed));
        assert!(sh.ended.load(Acquire));
        assert_eq!(sh.position.load(Relaxed), 1003);
    }
}
