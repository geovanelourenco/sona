//! Compilação da configuração em um plano de processamento.
//!
//! O plano é imutável e já traz tudo calculado (coeficientes de filtro, ganhos
//! lineares, ordem dos buses). Ele é montado fora da thread de áudio, onde
//! alocar é permitido, e entregue ao motor por uma fila sem trava.

use crate::biquad::Coefs;
use crate::config::*;
use crate::dynamics::{CompParams, GateParams, LimiterParams};
use crate::level::{db_to_gain, pan_mono, pan_stereo};
use serde::Serialize;

/// Formato fixo do motor. Mudar o `Layout` exige recriar o motor.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub sample_rate: f32,
    /// Maior bloco aceito sem fatiar.
    pub max_block: usize,
    /// Quantidade de canais na lista plana de entradas.
    pub inputs: usize,
    /// Quantidade de canais na lista plana de saídas.
    pub outputs: usize,
    pub max_channels: usize,
    pub max_buses: usize,
    /// Domínio (dispositivo) de cada entrada. Vazio = desconhecido. Serve para
    /// avisar de laço externo (USB da mesa entrando e voltando para a mesa).
    pub input_domains: Vec<u32>,
    pub output_domains: Vec<u32>,
}

impl Layout {
    pub fn new(sample_rate: f32, max_block: usize, inputs: usize, outputs: usize) -> Self {
        Layout {
            sample_rate,
            max_block,
            inputs,
            outputs,
            max_channels: 64,
            max_buses: 16,
            input_domains: Vec::new(),
            output_domains: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Issue {
    pub code: &'static str,
    pub path: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Validation {
    pub errors: Vec<Issue>,
    pub warnings: Vec<Issue>,
}

impl Validation {
    fn err(&mut self, code: &'static str, path: impl Into<String>, message: impl Into<String>) {
        self.errors.push(Issue { code, path: path.into(), message: message.into() });
    }
    fn warn(&mut self, code: &'static str, path: impl Into<String>, message: impl Into<String>) {
        self.warnings.push(Issue { code, path: path.into(), message: message.into() });
    }
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ChannelPlan {
    pub source: SourceMap,
    pub trim: f32,
    pub gate: GateParams,
    /// Filtros ativos (passa-altas e bandas de EQ), em ordem.
    pub filters: [Coefs; EQ_BANDS + 1],
    pub filter_count: usize,
    pub comp: CompParams,
    pub fader: f32,
    pub pan_l: f32,
    pub pan_r: f32,
    /// Ganho do envio para cada bus (denso, índice = bus). Zero = sem envio.
    pub send_gain: Vec<f32>,
    pub send_pre: Vec<bool>,
}

#[derive(Clone, Debug)]
pub(crate) struct MonitorPlan {
    pub follow: Option<usize>,
    /// Máscara de solo por canal (denso, índice = canal).
    pub solo: Vec<bool>,
    pub solo_active: bool,
    pub afl: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct BusPlan {
    pub enabled: bool,
    pub gain: f32,
    pub limiter: LimiterParams,
    pub outputs: Vec<OutputPair>,
    /// Ganho de envio para outros buses (denso, índice = bus).
    pub send_gain: Vec<f32>,
    pub monitor: Option<MonitorPlan>,
}

/// Plano pronto para o motor. Ver [`compile`].
#[derive(Clone, Debug)]
pub struct Plan {
    pub(crate) channels: Vec<ChannelPlan>,
    pub(crate) buses: Vec<BusPlan>,
    /// Ordem topológica: quem alimenta vem antes de quem é alimentado.
    pub(crate) bus_order: Vec<usize>,
}

impl Plan {
    pub fn empty() -> Plan {
        Plan { channels: Vec::new(), buses: Vec::new(), bus_order: Vec::new() }
    }
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }
    pub fn bus_count(&self) -> usize {
        self.buses.len()
    }
}

/// Verifica a configuração sem montar o plano.
pub fn validate(cfg: &MixerConfig, layout: &Layout) -> Validation {
    let mut v = Validation::default();
    let nb = cfg.buses.len();
    if cfg.channels.len() > layout.max_channels {
        v.err("too_many_channels", "/channels", format!("Limite de {} canais.", layout.max_channels));
    }
    if nb > layout.max_buses {
        v.err("too_many_buses", "/buses", format!("Limite de {} buses.", layout.max_buses));
    }
    for (i, ch) in cfg.channels.iter().enumerate() {
        for input in ch.source.inputs() {
            if input >= layout.inputs {
                v.err("input_out_of_range", format!("/channels/{i}/source"), format!("Entrada {input} não existe."));
            }
        }
        let mut seen = vec![false; nb];
        for (k, s) in ch.sends.iter().enumerate() {
            if s.bus >= nb {
                v.err("bus_out_of_range", format!("/channels/{i}/sends/{k}"), format!("Bus {} não existe.", s.bus));
            } else if std::mem::replace(&mut seen[s.bus], true) {
                v.err("duplicate_send", format!("/channels/{i}/sends/{k}"), "Dois envios para o mesmo bus.");
            }
        }
    }
    for (b, bus) in cfg.buses.iter().enumerate() {
        for (k, o) in bus.outputs.iter().enumerate() {
            if o.left >= layout.outputs || o.right >= layout.outputs {
                v.err("output_out_of_range", format!("/buses/{b}/outputs/{k}"), "Saída não existe.");
            }
        }
        for (k, s) in bus.bus_sends.iter().enumerate() {
            if s.bus >= nb {
                v.err("bus_out_of_range", format!("/buses/{b}/bus_sends/{k}"), format!("Bus {} não existe.", s.bus));
            } else if s.bus == b {
                v.err("cycle", format!("/buses/{b}/bus_sends/{k}"), "Um bus não pode enviar para ele mesmo.");
            }
        }
        if let Some(m) = &bus.monitor {
            if bus.kind != BusKind::Monitor {
                v.err("monitor_on_program_bus", format!("/buses/{b}/monitor"), "Solo e acompanhamento só existem em bus de monitor.");
            }
            if let Some(f) = m.follow {
                if f >= nb {
                    v.err("bus_out_of_range", format!("/buses/{b}/monitor/follow"), format!("Bus {f} não existe."));
                } else if f == b {
                    v.err("cycle", format!("/buses/{b}/monitor/follow"), "Um monitor não pode acompanhar ele mesmo.");
                }
            }
            for &c in &m.solo {
                if c >= cfg.channels.len() {
                    v.err("channel_out_of_range", format!("/buses/{b}/monitor/solo"), format!("Canal {c} não existe."));
                }
            }
        }
    }
    if v.is_ok() && bus_order(cfg).is_none() {
        v.err("cycle", "/buses", "A ligação entre buses forma um ciclo (realimentação).");
    }
    if v.is_ok() {
        console_loop_warnings(cfg, layout, &mut v);
    }
    v
}

fn bus_edges(cfg: &MixerConfig) -> Vec<(usize, usize)> {
    let mut e = Vec::new();
    for (b, bus) in cfg.buses.iter().enumerate() {
        for s in &bus.bus_sends {
            e.push((b, s.bus));
        }
        if let Some(f) = bus.monitor.as_ref().and_then(|m| m.follow) {
            e.push((f, b));
        }
    }
    e
}

/// Ordenação topológica (Kahn). `None` se houver ciclo.
fn bus_order(cfg: &MixerConfig) -> Option<Vec<usize>> {
    let n = cfg.buses.len();
    let edges = bus_edges(cfg);
    let mut indeg = vec![0usize; n];
    for &(_, t) in &edges {
        indeg[t] += 1;
    }
    let mut ready: Vec<usize> = (0..n).filter(|&b| indeg[b] == 0).rev().collect();
    let mut order = Vec::with_capacity(n);
    while let Some(b) = ready.pop() {
        order.push(b);
        for &(s, t) in &edges {
            if s == b {
                indeg[t] -= 1;
                if indeg[t] == 0 {
                    ready.push(t);
                }
            }
        }
    }
    (order.len() == n).then_some(order)
}

/// Avisa quando um canal que vem de um dispositivo chega, por qualquer
/// caminho, a um retorno de PA ligado que sai para o mesmo dispositivo.
fn console_loop_warnings(cfg: &MixerConfig, layout: &Layout, v: &mut Validation) {
    if layout.input_domains.is_empty() || layout.output_domains.is_empty() {
        return;
    }
    let Some(order) = bus_order(cfg) else { return };
    let nb = cfg.buses.len();
    for (i, ch) in cfg.channels.iter().enumerate() {
        let domains: Vec<u32> = ch.source.inputs().filter_map(|x| layout.input_domains.get(x).copied()).collect();
        if domains.is_empty() {
            continue;
        }
        let mut reach = vec![false; nb];
        for s in ch.sends.iter().filter(|s| s.enabled && s.level_db > crate::level::OFF_DB) {
            reach[s.bus] = true;
        }
        for &b in &order {
            if reach[b] {
                for s in &cfg.buses[b].bus_sends {
                    reach[s.bus] = true;
                }
            }
        }
        for (b, bus) in cfg.buses.iter().enumerate() {
            // Só o retorno de PA volta para a mesa física. Um bridge de rede que
            // manda mídia e recebe a transmissão é o uso normal (FR-08).
            if !reach[b] || !bus.enabled || bus.kind != BusKind::PaReturn {
                continue;
            }
            let hits = bus
                .outputs
                .iter()
                .any(|o| [o.left, o.right].iter().any(|x| layout.output_domains.get(*x).is_some_and(|d| domains.contains(d))));
            if hits {
                v.warn(
                    "possible_console_loop",
                    format!("/channels/{i}"),
                    format!(
                        "O canal \"{}\" vem do mesmo dispositivo para onde o bus \"{}\" volta. Se a mesa devolver esse retorno para o mesmo canal, haverá realimentação.",
                        ch.name, bus.name
                    ),
                );
            }
        }
    }
}

/// Monta o plano. Falha se a configuração tiver erros; avisos não impedem.
pub fn compile(cfg: &MixerConfig, layout: &Layout) -> Result<(Plan, Validation), Validation> {
    let v = validate(cfg, layout);
    if !v.is_ok() {
        return Err(v);
    }
    let sr = layout.sample_rate;
    let nb = cfg.buses.len();
    let nc = cfg.channels.len();

    let channels = cfg
        .channels
        .iter()
        .map(|ch| {
            let mut filters = [Coefs::IDENTITY; EQ_BANDS + 1];
            let mut n = 0;
            if ch.eq.enabled {
                if ch.eq.high_pass_enabled {
                    filters[n] = Coefs::high_pass(ch.eq.high_pass_hz, std::f32::consts::FRAC_1_SQRT_2, sr);
                    n += 1;
                }
                for band in ch.eq.bands.iter().filter(|b| b.enabled && b.gain_db != 0.0) {
                    filters[n] = match band.kind {
                        EqBandKind::Peaking => Coefs::peaking(band.freq_hz, band.q, band.gain_db, sr),
                        EqBandKind::LowShelf => Coefs::low_shelf(band.freq_hz, band.q, band.gain_db, sr),
                        EqBandKind::HighShelf => Coefs::high_shelf(band.freq_hz, band.q, band.gain_db, sr),
                    };
                    n += 1;
                }
            }
            let g = &ch.gate;
            let c = &ch.comp;
            let (pan_l, pan_r) = if ch.source.is_stereo() { pan_stereo(ch.pan) } else { pan_mono(ch.pan) };
            let mut send_gain = vec![0.0; nb];
            let mut send_pre = vec![false; nb];
            if !ch.mute {
                for s in ch.sends.iter().filter(|s| s.enabled) {
                    send_gain[s.bus] = db_to_gain(s.level_db);
                    send_pre[s.bus] = s.pre_fader;
                }
            }
            ChannelPlan {
                source: ch.source,
                trim: db_to_gain(ch.trim_db) * if ch.polarity_invert { -1.0 } else { 1.0 },
                gate: if g.enabled {
                    GateParams::new(g.threshold_db, g.range_db, g.attack_ms, g.hold_ms, g.release_ms, sr)
                } else {
                    GateParams::BYPASS
                },
                filters,
                filter_count: n,
                comp: if c.enabled {
                    CompParams::new(c.threshold_db, c.ratio, c.knee_db, c.attack_ms, c.release_ms, c.makeup_db, sr)
                } else {
                    CompParams::BYPASS
                },
                fader: if ch.mute { 0.0 } else { db_to_gain(ch.fader_db) },
                pan_l,
                pan_r,
                send_gain,
                send_pre,
            }
        })
        .collect();

    let buses = cfg
        .buses
        .iter()
        .map(|bus| {
            let mut send_gain = vec![0.0; nb];
            for s in &bus.bus_sends {
                send_gain[s.bus] = db_to_gain(s.level_db);
            }
            let monitor = bus.monitor.as_ref().map(|m| {
                let mut solo = vec![false; nc];
                for &c in &m.solo {
                    solo[c] = true;
                }
                MonitorPlan { follow: m.follow, solo_active: !m.solo.is_empty(), solo, afl: m.solo_mode == SoloMode::Afl }
            });
            BusPlan {
                enabled: bus.enabled,
                gain: if bus.mute { 0.0 } else { db_to_gain(bus.fader_db) },
                limiter: if bus.limiter.enabled {
                    LimiterParams::new(bus.limiter.ceiling_db, bus.limiter.release_ms, sr)
                } else {
                    LimiterParams::BYPASS
                },
                outputs: bus.outputs.clone(),
                send_gain,
                monitor,
            }
        })
        .collect();

    let bus_order = bus_order(cfg).expect("validado acima");
    Ok((Plan { channels, buses, bus_order }, v))
}
