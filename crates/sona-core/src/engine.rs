//! Motor de tempo real.
//!
//! [`Engine::process`] roda na thread de áudio: não aloca, não bloqueia, não
//! faz I/O. Mudanças chegam como planos novos por uma fila SPSC sem trava e são
//! aplicadas na fronteira do bloco. O plano antigo volta pela fila de retorno
//! para ser liberado fora da thread de áudio.

use crate::biquad::State as Biquad;
use crate::config::{SourceMap, EQ_BANDS};
use crate::dynamics::{CompState, GateState, LimiterState};
use crate::meters::Meters;
use crate::plan::{Layout, Plan};
use crate::smooth::Smoother;
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::Arc;

/// Rampa de ganho: 10 ms.
const RAMP_MS: f32 = 10.0;
const PLAN_QUEUE: usize = 64;

struct ChannelState {
    trim: Smoother,
    gate: GateState,
    filters: [[Biquad; 2]; EQ_BANDS + 1],
    comp: CompState,
    fader: Smoother,
    pan_l: Smoother,
    pan_r: Smoother,
    sends: Vec<Smoother>,
}

struct BusState {
    gain: Smoother,
    limiter: LimiterState,
    sends: Vec<Smoother>,
}

/// Lado de controle: envia planos e recolhe os antigos.
pub struct EngineHandle {
    tx: Producer<Box<Plan>>,
    returned: Consumer<Box<Plan>>,
    pub meters: Arc<Meters>,
}

impl EngineHandle {
    /// Envia um plano. Libera antes os planos que o motor já devolveu.
    /// Devolve o plano se a fila estiver cheia (motor parado ou travado).
    pub fn submit(&mut self, plan: Plan) -> Result<(), Box<Plan>> {
        self.collect();
        self.tx.push(Box::new(plan)).map_err(|rtrb::PushError::Full(p)| p)
    }

    /// Libera planos devolvidos (fora da thread de áudio).
    pub fn collect(&mut self) {
        while self.returned.pop().is_ok() {}
    }
}

pub struct Engine {
    layout: Layout,
    plan: Box<Plan>,
    rx: Consumer<Box<Plan>>,
    ret: Producer<Box<Plan>>,
    meters: Arc<Meters>,
    ch: Vec<ChannelState>,
    bus: Vec<BusState>,
    pre: [Vec<f32>; 2],
    post: [Vec<f32>; 2],
    bus_buf: Vec<[Vec<f32>; 2]>,
    solo_buf: Vec<[Vec<f32>; 2]>,
    /// Planos que não couberam na fila de retorno. Ver `swap_plan`.
    dropped_returns: u64,
}

impl Engine {
    /// Cria o motor com toda a memória que ele vai usar.
    pub fn new(layout: Layout, plan: Plan) -> (Engine, EngineHandle) {
        let (tx, rx) = RingBuffer::new(PLAN_QUEUE);
        let (ret, returned) = RingBuffer::new(PLAN_QUEUE);
        let meters = Arc::new(Meters::new(layout.max_channels, layout.max_buses));
        let ramp = (RAMP_MS * 0.001 * layout.sample_rate) as u32;
        let nb = layout.max_buses;
        let blk = layout.max_block;
        let ch = (0..layout.max_channels)
            .map(|_| ChannelState {
                trim: Smoother::new(0.0, ramp),
                gate: GateState::default(),
                filters: Default::default(),
                comp: CompState::default(),
                fader: Smoother::new(0.0, ramp),
                pan_l: Smoother::new(0.0, ramp),
                pan_r: Smoother::new(0.0, ramp),
                sends: vec![Smoother::new(0.0, ramp); nb],
            })
            .collect();
        let bus = (0..nb)
            .map(|_| BusState {
                gain: Smoother::new(0.0, ramp),
                limiter: LimiterState::default(),
                sends: vec![Smoother::new(0.0, ramp); nb],
            })
            .collect();
        let buf2 = || [vec![0.0f32; blk], vec![0.0f32; blk]];
        let mut engine = Engine {
            plan: Box::new(Plan::empty()),
            rx,
            ret,
            meters: meters.clone(),
            ch,
            bus,
            pre: buf2(),
            post: buf2(),
            bus_buf: (0..nb).map(|_| buf2()).collect(),
            solo_buf: (0..nb).map(|_| buf2()).collect(),
            dropped_returns: 0,
            layout,
        };
        engine.plan = Box::new(plan);
        engine.apply_targets(true);
        (engine, EngineHandle { tx, returned, meters })
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn dropped_returns(&self) -> u64 {
        self.dropped_returns
    }

    fn swap_plan(&mut self) {
        let mut changed = false;
        while let Ok(p) = self.rx.pop() {
            let old = std::mem::replace(&mut self.plan, p);
            if let Err(rtrb::PushError::Full(old)) = self.ret.push(old) {
                // Fila de retorno cheia: o controle não está recolhendo. Liberar
                // aqui é a única saída; conta para diagnóstico.
                self.dropped_returns += 1;
                drop(old);
            }
            changed = true;
        }
        if changed {
            self.apply_targets(false);
        }
    }

    fn apply_targets(&mut self, immediate: bool) {
        let plan = &*self.plan;
        let set = |s: &mut Smoother, v: f32| if immediate { s.reset(v) } else { s.set_target(v) };
        for (i, st) in self.ch.iter_mut().enumerate() {
            match plan.channels.get(i) {
                Some(p) => {
                    set(&mut st.trim, p.trim);
                    set(&mut st.fader, p.fader);
                    set(&mut st.pan_l, p.pan_l);
                    set(&mut st.pan_r, p.pan_r);
                    for (b, s) in st.sends.iter_mut().enumerate() {
                        set(s, p.send_gain.get(b).copied().unwrap_or(0.0));
                    }
                }
                None => {
                    set(&mut st.fader, 0.0);
                    for s in st.sends.iter_mut() {
                        set(s, 0.0);
                    }
                }
            }
        }
        for (b, st) in self.bus.iter_mut().enumerate() {
            match plan.buses.get(b) {
                Some(p) => {
                    set(&mut st.gain, p.gain);
                    for (t, s) in st.sends.iter_mut().enumerate() {
                        set(s, p.send_gain.get(t).copied().unwrap_or(0.0));
                    }
                }
                None => set(&mut st.gain, 0.0),
            }
        }
    }

    /// Processa um bloco. `inputs` e `outputs` seguem a lista plana do
    /// [`Layout`]. Entrada ausente vale silêncio; saída ausente é ignorada.
    /// Blocos maiores que `max_block` são fatiados.
    pub fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], frames: usize) {
        self.swap_plan();
        for o in outputs.iter_mut() {
            let n = frames.min(o.len());
            o[..n].fill(0.0);
        }
        let mut off = 0;
        while off < frames {
            let n = (frames - off).min(self.layout.max_block);
            self.process_slice(inputs, outputs, off, n);
            off += n;
        }
        self.meters.blocks.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn process_slice(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]], off: usize, n: usize) {
        let plan = &*self.plan;
        let nb = plan.buses.len();
        for b in 0..nb {
            for k in 0..2 {
                self.bus_buf[b][k][..n].fill(0.0);
                self.solo_buf[b][k][..n].fill(0.0);
            }
        }

        // ── Canais ──
        let read = |idx: usize, i: usize| -> f32 { inputs.get(idx).and_then(|s| s.get(off + i)).copied().unwrap_or(0.0) };
        for (c, p) in plan.channels.iter().enumerate() {
            let st = &mut self.ch[c];
            let [pre_l, pre_r] = &mut self.pre;
            let [post_l, post_r] = &mut self.post;
            let (stereo, il, ir) = match p.source {
                SourceMap::None => (false, usize::MAX, usize::MAX),
                SourceMap::Mono { input } => (false, input, input),
                SourceMap::Stereo { left, right } => (true, left, right),
            };
            let mut peak = 0.0f32;
            for i in 0..n {
                let t = st.trim.next_value();
                let mut l = read(il, i) * t;
                let mut r = if stereo { read(ir, i) * t } else { l };
                let g = st.gate.tick(&p.gate, l.abs().max(r.abs()));
                l *= g;
                r *= g;
                for f in 0..p.filter_count {
                    let [fl, fr] = &mut st.filters[f];
                    l = fl.process(&p.filters[f], l);
                    if stereo {
                        r = fr.process(&p.filters[f], r);
                    }
                }
                if !stereo {
                    r = l;
                }
                let cg = st.comp.tick(&p.comp, l.abs().max(r.abs()));
                l *= cg;
                r *= cg;
                let (sl, sr) = (l * st.pan_l.next_value(), r * st.pan_r.next_value());
                let fg = st.fader.next_value();
                pre_l[i] = sl;
                pre_r[i] = sr;
                post_l[i] = sl * fg;
                post_r[i] = sr * fg;
                peak = peak.max(post_l[i].abs()).max(post_r[i].abs());
            }
            Meters::put(&self.meters.channel_peak[c], peak);
            Meters::put(&self.meters.channel_gr_db[c], st.comp.gr_db());
            Meters::put(&self.meters.channel_gate[c], st.gate.gain());

            for b in 0..nb {
                let s = &mut st.sends[b];
                if s.is_settled() && s.current() == 0.0 {
                    continue;
                }
                let (src_l, src_r) = if p.send_pre[b] { (&*pre_l, &*pre_r) } else { (&*post_l, &*post_r) };
                let [bl, br] = &mut self.bus_buf[b];
                for i in 0..n {
                    let g = s.next_value();
                    bl[i] += src_l[i] * g;
                    br[i] += src_r[i] * g;
                }
            }
            for (b, bp) in plan.buses.iter().enumerate() {
                let Some(m) = &bp.monitor else { continue };
                if !m.solo.get(c).copied().unwrap_or(false) {
                    continue;
                }
                let (src_l, src_r) = if m.afl { (&*post_l, &*post_r) } else { (&*pre_l, &*pre_r) };
                let [sl, sr] = &mut self.solo_buf[b];
                for i in 0..n {
                    sl[i] += src_l[i];
                    sr[i] += src_r[i];
                }
            }
        }

        // ── Buses, em ordem topológica ──
        for &b in &plan.bus_order {
            let bp = &plan.buses[b];
            let mut buf = std::mem::take(&mut self.bus_buf[b]);
            if let Some(m) = &bp.monitor {
                if m.solo_active {
                    buf[0][..n].copy_from_slice(&self.solo_buf[b][0][..n]);
                    buf[1][..n].copy_from_slice(&self.solo_buf[b][1][..n]);
                } else if let Some(f) = m.follow {
                    let src = &self.bus_buf[f];
                    for k in 0..2 {
                        for i in 0..n {
                            buf[k][i] += src[k][i];
                        }
                    }
                }
            }
            let st = &mut self.bus[b];
            let (mut pl, mut pr, mut lim_min) = (0.0f32, 0.0f32, 1.0f32);
            for i in 0..n {
                let g = st.gain.next_value();
                let (l, r) = (buf[0][i] * g, buf[1][i] * g);
                let lg = st.limiter.tick(&bp.limiter, l.abs().max(r.abs()));
                lim_min = lim_min.min(lg);
                buf[0][i] = l * lg;
                buf[1][i] = r * lg;
                pl = pl.max(buf[0][i].abs());
                pr = pr.max(buf[1][i].abs());
            }
            Meters::put(&self.meters.bus_peak[b][0], pl);
            Meters::put(&self.meters.bus_peak[b][1], pr);
            Meters::put(&self.meters.bus_limit_db[b], -crate::level::gain_to_db(lim_min));

            for t in 0..nb {
                let s = &mut st.sends[t];
                if s.is_settled() && s.current() == 0.0 {
                    continue;
                }
                let [tl, tr] = &mut self.bus_buf[t];
                for i in 0..n {
                    let g = s.next_value();
                    tl[i] += buf[0][i] * g;
                    tr[i] += buf[1][i] * g;
                }
            }

            if bp.enabled {
                for o in &bp.outputs {
                    for (k, idx) in [o.left, o.right].into_iter().enumerate() {
                        if let Some(out) = outputs.get_mut(idx) {
                            let end = (off + n).min(out.len());
                            if end > off {
                                for (y, x) in out[off..end].iter_mut().zip(&buf[k][..end - off]) {
                                    *y += *x;
                                }
                            }
                        }
                    }
                }
            }
            self.bus_buf[b] = buf;
        }
    }
}
