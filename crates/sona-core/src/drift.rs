//! Ponte entre dois domínios de relógio.
//!
//! Dois aparelhos com a mesma taxa nominal (48 kHz) nunca andam juntos: um
//! sempre corre algumas dezenas de ppm mais rápido. Sem correção, a fila entre
//! eles esvazia ou transborda em minutos (estalos). Aqui, quem lê da fila
//! reamostra com uma razão ajustada continuamente por um controlador PI que
//! mantém o nível da fila no alvo.
//!
//! - `Writer::push_*` roda no relógio de quem produz.
//! - `Reader::pull_*` roda no relógio de quem consome e faz a reamostragem
//!   (interpolação cúbica Catmull-Rom).
//!
//! Os dois lados são de tempo real: sem alocação, sem trava (fila SPSC rtrb,
//! estatísticas em atômicos). Serve para dispositivo secundário e para rede
//! (o alvo maior absorve o jitter dos pacotes).

use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering::Acquire, Ordering::Relaxed, Ordering::Release};
use std::sync::Arc;
use std::time::Instant;

/// Correção máxima de velocidade: 2000 ppm (0,2 %). Relógios de áudio reais
/// ficam abaixo de 200 ppm; acima disso a ponte trata como falha e conta.
pub const MAX_CORRECTION: f64 = 0.002;

/// Estatísticas lidas pelo controle (sem trava).
#[derive(Default)]
pub struct DriftStats {
    pub underruns: AtomicU64,
    pub overruns: AtomicU64,
    /// Correção atual em ppm × 1000 (positivo: quem lê consome mais rápido).
    pub correction_ppb: AtomicI64,
    /// Nível da fila em quadros (média).
    pub fill_frames: AtomicU64,
}

/// Progresso do produtor, para o leitor estimar o nível da fila sem o dente de
/// serra dos blocos: total escrito, instante e tamanho do último bloco.
#[derive(Default)]
struct Progress {
    written: AtomicU64,
    at_ns: AtomicU64,
    last_block: AtomicU64,
}

impl DriftStats {
    pub fn correction_ppm(&self) -> f64 {
        self.correction_ppb.load(Relaxed) as f64 / 1000.0
    }
}

pub struct Writer {
    prod: Producer<f32>,
    channels: usize,
    stats: Arc<DriftStats>,
    progress: Arc<Progress>,
    epoch: Instant,
}

pub struct Reader {
    cons: Consumer<f32>,
    channels: usize,
    /// Histórico para a interpolação: 4 quadros × canais (x[n-1], x[n], x[n+1], x[n+2]).
    hist: Vec<f32>,
    pos: f64,
    playing: bool,
    target: f64,
    fill_ema: f64,
    integral: f64,
    ratio: f64,
    sample_rate: f64,
    stats: Arc<DriftStats>,
    progress: Arc<Progress>,
    consumed: u64,
    epoch: Instant,
}

/// Cria a ponte. `target_ms`: nível desejado da fila (latência adicionada);
/// a capacidade é 4× o alvo, no mínimo 4096 quadros.
pub fn bridge(channels: usize, sample_rate: f64, target_ms: f64) -> (Writer, Reader, Arc<DriftStats>) {
    let channels = channels.max(1);
    let target = (target_ms * 0.001 * sample_rate).max(32.0);
    let capacity = ((target * 4.0) as usize).max(4096);
    let (prod, cons) = RingBuffer::new(capacity * channels);
    let stats = Arc::new(DriftStats::default());
    let progress = Arc::new(Progress::default());
    let epoch = Instant::now();
    (
        Writer { prod, channels, stats: stats.clone(), progress: progress.clone(), epoch },
        Reader {
            cons,
            channels,
            hist: vec![0.0; 4 * channels],
            pos: 0.0,
            playing: false,
            target,
            fill_ema: 0.0,
            integral: 0.0,
            ratio: 1.0,
            sample_rate,
            stats: stats.clone(),
            progress,
            consumed: 0,
            epoch,
        },
        stats,
    )
}

impl Writer {
    pub fn channels(&self) -> usize {
        self.channels
    }

    fn now_s(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64()
    }

    fn record(&self, frames: usize, now_s: f64) {
        let p = &self.progress;
        p.last_block.store(frames as u64, Relaxed);
        p.at_ns.store((now_s * 1e9) as u64, Relaxed);
        p.written.fetch_add(frames as u64, Release);
    }

    /// Escreve quadros intercalados. Fila cheia: descarta o resto e conta.
    pub fn push_interleaved(&mut self, data: &[f32]) {
        let t = self.now_s();
        self.push_interleaved_at(data, t);
    }

    /// Igual a `push_interleaved`, com o instante dado (segundos; para testes).
    pub fn push_interleaved_at(&mut self, data: &[f32], now_s: f64) {
        let frames = data.len() / self.channels;
        let free = self.prod.slots() / self.channels;
        let n = frames.min(free);
        if n < frames {
            self.stats.overruns.fetch_add(1, Relaxed);
        }
        if let Ok(mut chunk) = self.prod.write_chunk_uninit(n * self.channels) {
            let (a, b) = chunk.as_mut_slices();
            for (dst, src) in a.iter_mut().chain(b.iter_mut()).zip(&data[..n * self.channels]) {
                dst.write(*src);
            }
            // SAFETY: todos os n × canais elementos foram escritos acima.
            unsafe { chunk.commit_all() };
        }
        self.record(n, now_s);
    }

    /// Escreve quadros planares (um slice por canal; canal ausente = silêncio).
    pub fn push_planar(&mut self, data: &[&[f32]], frames: usize) {
        let now_s = self.now_s();
        let ch = self.channels;
        let free = self.prod.slots() / ch;
        let n = frames.min(free);
        if n < frames {
            self.stats.overruns.fetch_add(1, Relaxed);
        }
        if let Ok(mut chunk) = self.prod.write_chunk_uninit(n * ch) {
            let (a, b) = chunk.as_mut_slices();
            for (k, dst) in a.iter_mut().chain(b.iter_mut()).enumerate() {
                let (i, c) = (k / ch, k % ch);
                dst.write(data.get(c).and_then(|s| s.get(i)).copied().unwrap_or(0.0));
            }
            // SAFETY: todos os n × canais elementos foram escritos acima.
            unsafe { chunk.commit_all() };
        }
        self.record(n, now_s);
    }
}

impl Reader {
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// Latência média adicionada pela fila, em ms.
    pub fn latency_ms(&self) -> f64 {
        self.fill_ema / self.sample_rate * 1000.0
    }

    fn available(&self) -> usize {
        self.cons.slots() / self.channels
    }

    /// Avança um quadro no histórico. `false` = fila vazia.
    fn advance(&mut self) -> bool {
        let ch = self.channels;
        if self.cons.slots() < ch {
            return false;
        }
        self.hist.copy_within(ch.., 0);
        let base = 3 * ch;
        if let Ok(chunk) = self.cons.read_chunk(ch) {
            let (a, b) = chunk.as_slices();
            for (k, v) in a.iter().chain(b).enumerate() {
                self.hist[base + k] = *v;
            }
            chunk.commit_all();
        }
        self.consumed += 1;
        true
    }

    /// Nível estimado da fila agora, em quadros. Soma ao que o produtor já
    /// escreveu o que ele terá produzido desde o último bloco (até um bloco),
    /// o que tira o dente de serra de quando os dois blocos têm o mesmo tamanho.
    fn estimated_fill(&self, now_s: f64) -> f64 {
        let p = &self.progress;
        let written = p.written.load(Acquire) as f64;
        let at = p.at_ns.load(Relaxed) as f64 * 1e-9;
        let block = p.last_block.load(Relaxed) as f64;
        let since = ((now_s - at) * self.sample_rate).clamp(0.0, block);
        // Quadros no histórico ainda não tocados: entre x[n] e x[n+2].
        written + since - block - self.consumed as f64 + (2.0 - self.pos)
    }

    /// Ajusta a razão de leitura pelo nível da fila. Chamado uma vez por bloco.
    fn control(&mut self, frames: usize, now_s: f64) {
        let fill = self.estimated_fill(now_s);
        let dt = frames as f64 / self.sample_rate;
        // Média de ~1 s: tira o jitter de chegada.
        let a = 1.0 - (-dt / 1.0).exp();
        if self.fill_ema == 0.0 {
            self.fill_ema = fill;
        }
        self.fill_ema += a * (fill - self.fill_ema);
        let err = (self.fill_ema - self.target) / self.sample_rate; // segundos
                                                                    // PI criticamente amortecido (ωn = 0,05 rad/s, assenta em ~1 min):
                                                                    // corrige deriva sem modular a afinação. 100 ppm pedem ~1 ms de erro
                                                                    // até a integral assumir.
        const KP: f64 = 0.1;
        const KI: f64 = 0.0025;
        if self.playing {
            self.integral = (self.integral + err * dt).clamp(-MAX_CORRECTION / KI, MAX_CORRECTION / KI);
        }
        let c = (KP * err + KI * self.integral).clamp(-MAX_CORRECTION, MAX_CORRECTION);
        self.ratio = 1.0 + c;
        self.stats.correction_ppb.store((c * 1e9) as i64, Relaxed);
        self.stats.fill_frames.store(self.fill_ema.max(0.0) as u64, Relaxed);
    }

    #[inline]
    fn sample(&self, c: usize) -> f32 {
        let ch = self.channels;
        let (y0, y1, y2, y3) = (self.hist[c], self.hist[ch + c], self.hist[2 * ch + c], self.hist[3 * ch + c]);
        let t = self.pos as f32;
        // Catmull-Rom entre y1 e y2.
        let a = -0.5 * y0 + 1.5 * y1 - 1.5 * y2 + 0.5 * y3;
        let b = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let cc = -0.5 * y0 + 0.5 * y2;
        ((a * t + b) * t + cc) * t + y1
    }

    /// Lê `frames` quadros reamostrados para `out` (um slice por canal; canais
    /// a mais ficam em silêncio). Fila vazia: silêncio, conta e volta a encher.
    pub fn pull_planar(&mut self, out: &mut [&mut [f32]], frames: usize) {
        let t = self.epoch.elapsed().as_secs_f64();
        self.pull_planar_at(out, frames, t);
    }

    /// Igual a `pull_planar`, com o instante dado (segundos; para testes).
    pub fn pull_planar_at(&mut self, out: &mut [&mut [f32]], frames: usize, now_s: f64) {
        self.control(frames, now_s);
        if !self.playing {
            if self.available() as f64 >= self.target {
                self.playing = true;
                self.pos = 0.0;
                // Enche o histórico (x[n-1]..x[n+2]) antes de interpolar.
                for _ in 0..3 {
                    self.advance();
                }
            } else {
                for o in out.iter_mut() {
                    let n = frames.min(o.len());
                    o[..n].fill(0.0);
                }
                return;
            }
        }
        for i in 0..frames {
            while self.pos >= 1.0 {
                if !self.advance() {
                    self.stats.underruns.fetch_add(1, Relaxed);
                    self.playing = false;
                    for o in out.iter_mut() {
                        let n = frames.min(o.len());
                        if i < n {
                            o[i..n].fill(0.0);
                        }
                    }
                    return;
                }
                self.pos -= 1.0;
            }
            for (c, o) in out.iter_mut().enumerate() {
                if let Some(s) = o.get_mut(i) {
                    *s = if c < self.channels { self.sample(c) } else { 0.0 };
                }
            }
            self.pos += self.ratio;
        }
    }

    /// Igual a `pull_planar`, saída intercalada com `out_channels` canais.
    pub fn pull_interleaved(&mut self, out: &mut [f32], out_channels: usize, scratch: &mut [Vec<f32>]) {
        let frames = out.len() / out_channels.max(1);
        let mut refs: [&mut [f32]; 16] = Default::default();
        let mut n = 0;
        for (r, s) in refs.iter_mut().zip(scratch.iter_mut()) {
            let len = frames.min(s.len());
            *r = &mut s[..len];
            n += 1;
        }
        self.pull_planar(&mut refs[..n], frames);
        for i in 0..frames {
            for c in 0..out_channels {
                out[i * out_channels + c] = refs.get(c).and_then(|s| s.get(i)).copied().unwrap_or(0.0);
            }
        }
    }
}
