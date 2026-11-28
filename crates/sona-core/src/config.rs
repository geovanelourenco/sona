//! Configuração do mixer: dados puros, serializáveis. A UI e a persistência
//! trabalham com estes tipos. O motor nunca lê estes tipos diretamente: eles
//! passam por [`crate::plan::compile`] primeiro.

use serde::{Deserialize, Serialize};

/// Versão do formato de configuração. Aumente e escreva uma migração no
/// servidor quando mudar o significado de um campo.
pub const CONFIG_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MixerConfig {
    pub version: u32,
    pub channels: Vec<ChannelConfig>,
    pub buses: Vec<BusConfig>,
}

impl Default for MixerConfig {
    fn default() -> Self {
        MixerConfig { version: CONFIG_VERSION, channels: Vec::new(), buses: Vec::new() }
    }
}

/// De onde o canal lê o sinal. Os índices apontam para a lista plana de
/// entradas do motor (canais físicos e de rede, ver [`crate::plan::Layout`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceMap {
    None,
    Mono { input: usize },
    Stereo { left: usize, right: usize },
}

impl SourceMap {
    pub fn is_stereo(&self) -> bool {
        matches!(self, SourceMap::Stereo { .. })
    }
    pub fn inputs(&self) -> impl Iterator<Item = usize> {
        let (a, b) = match *self {
            SourceMap::None => (None, None),
            SourceMap::Mono { input } => (Some(input), None),
            SourceMap::Stereo { left, right } => (Some(left), Some(right)),
        };
        a.into_iter().chain(b)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelConfig {
    pub name: String,
    /// Etiquetas livres ("Vocal", "Banda"…) para achar e filtrar canais.
    pub tags: Vec<String>,
    pub source: SourceMap,
    pub trim_db: f32,
    pub polarity_invert: bool,
    pub gate: GateConfig,
    pub eq: EqConfig,
    pub comp: CompConfig,
    pub fader_db: f32,
    pub mute: bool,
    /// -1 (esquerda) a +1 (direita). Em fonte estéreo é balanço.
    pub pan: f32,
    pub sends: Vec<SendConfig>,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        ChannelConfig {
            name: String::new(),
            tags: Vec::new(),
            source: SourceMap::None,
            trim_db: 0.0,
            polarity_invert: false,
            gate: GateConfig::default(),
            eq: EqConfig::default(),
            comp: CompConfig::default(),
            fader_db: crate::level::OFF_DB,
            mute: false,
            pan: 0.0,
            sends: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GateConfig {
    pub enabled: bool,
    pub threshold_db: f32,
    pub range_db: f32,
    pub attack_ms: f32,
    pub hold_ms: f32,
    pub release_ms: f32,
}

impl Default for GateConfig {
    fn default() -> Self {
        GateConfig { enabled: false, threshold_db: -50.0, range_db: 60.0, attack_ms: 0.5, hold_ms: 50.0, release_ms: 150.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EqBandKind {
    Peaking,
    LowShelf,
    HighShelf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EqBand {
    pub enabled: bool,
    pub kind: EqBandKind,
    pub freq_hz: f32,
    pub gain_db: f32,
    pub q: f32,
}

impl Default for EqBand {
    fn default() -> Self {
        EqBand { enabled: true, kind: EqBandKind::Peaking, freq_hz: 1000.0, gain_db: 0.0, q: 0.7 }
    }
}

pub const EQ_BANDS: usize = 4;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EqConfig {
    pub enabled: bool,
    pub high_pass_enabled: bool,
    pub high_pass_hz: f32,
    pub bands: [EqBand; EQ_BANDS],
}

impl Default for EqConfig {
    fn default() -> Self {
        EqConfig {
            enabled: true,
            high_pass_enabled: false,
            high_pass_hz: 80.0,
            bands: [
                EqBand { kind: EqBandKind::LowShelf, freq_hz: 120.0, ..EqBand::default() },
                EqBand { freq_hz: 400.0, q: 1.0, ..EqBand::default() },
                EqBand { freq_hz: 2500.0, q: 1.0, ..EqBand::default() },
                EqBand { kind: EqBandKind::HighShelf, freq_hz: 8000.0, ..EqBand::default() },
            ],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompConfig {
    pub enabled: bool,
    pub threshold_db: f32,
    pub ratio: f32,
    pub knee_db: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub makeup_db: f32,
}

impl Default for CompConfig {
    fn default() -> Self {
        CompConfig { enabled: false, threshold_db: -18.0, ratio: 3.0, knee_db: 6.0, attack_ms: 10.0, release_ms: 120.0, makeup_db: 0.0 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SendConfig {
    pub bus: usize,
    pub enabled: bool,
    pub level_db: f32,
    /// Pré-fader: o envio não segue o fader do canal (segue o mute).
    pub pre_fader: bool,
}

impl Default for SendConfig {
    fn default() -> Self {
        SendConfig { bus: 0, enabled: true, level_db: 0.0, pre_fader: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusKind {
    /// Mix da transmissão (OBS).
    Broadcast,
    /// Retorno para a mesa física pelo USB. Desligado por padrão.
    PaReturn,
    Recording,
    /// Fone de um operador. Solo/PFL só afeta este tipo de bus.
    Monitor,
    /// Subgrupo interno.
    Group,
}

/// Par de canais de saída (índices na lista plana de saídas do motor).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputPair {
    pub left: usize,
    pub right: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BusSendConfig {
    pub bus: usize,
    pub level_db: f32,
}

impl Default for BusSendConfig {
    fn default() -> Self {
        BusSendConfig { bus: 0, level_db: 0.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoloMode {
    /// Pré-fader: escuta o canal processado, antes do fader.
    Pfl,
    /// Pós-fader: escuta o canal depois do fader e do pan.
    Afl,
}

/// O que um bus de monitor toca. Com solo ativo, o solo substitui o resto.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MonitorConfig {
    /// Bus que o operador acompanha quando não há solo (ex.: a transmissão).
    pub follow: Option<usize>,
    pub solo: Vec<usize>,
    pub solo_mode: SoloMode,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        MonitorConfig { follow: None, solo: Vec::new(), solo_mode: SoloMode::Pfl }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LimiterConfig {
    pub enabled: bool,
    pub ceiling_db: f32,
    pub release_ms: f32,
}

impl Default for LimiterConfig {
    fn default() -> Self {
        LimiterConfig { enabled: false, ceiling_db: -1.0, release_ms: 80.0 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BusConfig {
    pub name: String,
    pub kind: BusKind,
    /// Bus desligado não toca em nenhuma saída. Retorno de PA nasce desligado.
    pub enabled: bool,
    pub fader_db: f32,
    pub mute: bool,
    pub limiter: LimiterConfig,
    pub outputs: Vec<OutputPair>,
    pub bus_sends: Vec<BusSendConfig>,
    pub monitor: Option<MonitorConfig>,
}

impl Default for BusConfig {
    fn default() -> Self {
        BusConfig {
            name: String::new(),
            kind: BusKind::Group,
            enabled: true,
            fader_db: 0.0,
            mute: false,
            limiter: LimiterConfig::default(),
            outputs: Vec::new(),
            bus_sends: Vec::new(),
            monitor: None,
        }
    }
}

impl BusConfig {
    pub fn new(name: &str, kind: BusKind) -> Self {
        BusConfig {
            name: name.into(),
            kind,
            enabled: kind != BusKind::PaReturn,
            monitor: (kind == BusKind::Monitor).then(MonitorConfig::default),
            ..BusConfig::default()
        }
    }
}
