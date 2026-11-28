//! Estado do mixer no servidor: configuração atual, cenas, revisão,
//! persistência atômica e entrega de planos ao motor.

use crate::audio::IoLayout;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sona_core::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const STATE_FORMAT: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Persisted {
    pub format: u32,
    pub mixer: MixerConfig,
    #[serde(default)]
    pub scenes: BTreeMap<String, MixerConfig>,
}

pub struct Store {
    pub mixer: MixerConfig,
    pub scenes: BTreeMap<String, MixerConfig>,
    pub rev: u64,
    pub validation: Validation,
    pub io: IoLayout,
    pub layout: Layout,
    engine: EngineHandle,
    path: PathBuf,
    dirty: bool,
}

#[derive(Debug)]
pub enum PatchError {
    Invalid(String),
    Rejected(Validation),
}

impl std::fmt::Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatchError::Invalid(m) => f.write_str(m),
            PatchError::Rejected(v) => {
                let msgs: Vec<_> = v.errors.iter().map(|i| i.message.as_str()).collect();
                write!(f, "Configuração recusada: {}", msgs.join(" "))
            }
        }
    }
}

/// Configuração inicial para a lista de canais atual: um canal mono por
/// entrada do principal (até 24), um estéreo por par L/R dos secundários e das
/// 4 primeiras vagas de rede. Transmissão sai pela vaga de rede 1 (bridge do
/// OBS) e, se houver, também pelo primeiro secundário de saída.
pub fn default_mixer(io: &IoLayout) -> MixerConfig {
    use crate::audio::{NET_BASE, SECONDARY_BASE};
    let mut cfg = MixerConfig::default();
    let pair = |l: usize| OutputPair { left: l, right: l + 1 };
    let present_out = |i: usize| io.outputs.get(i).is_some_and(|p| p.present) && io.outputs.get(i + 1).is_some_and(|p| p.present);
    let mut broadcast = vec![pair(NET_BASE)];
    if present_out(SECONDARY_BASE) {
        broadcast.push(pair(SECONDARY_BASE));
    }
    let primary_pair = |l: usize| present_out(l).then(|| pair(l)).into_iter().collect::<Vec<_>>();
    cfg.buses = vec![
        BusConfig {
            outputs: broadcast,
            limiter: LimiterConfig { enabled: true, ceiling_db: -1.0, release_ms: 80.0 },
            ..BusConfig::new("Transmissão", BusKind::Broadcast)
        },
        BusConfig { outputs: primary_pair(0), ..BusConfig::new("Retorno PA", BusKind::PaReturn) },
        BusConfig { outputs: primary_pair(4), ..BusConfig::new("Gravação", BusKind::Recording) },
        BusConfig {
            outputs: primary_pair(6),
            monitor: Some(MonitorConfig { follow: Some(0), ..Default::default() }),
            ..BusConfig::new("Fone 1", BusKind::Monitor)
        },
        BusConfig {
            outputs: primary_pair(8),
            monitor: Some(MonitorConfig { follow: Some(0), ..Default::default() }),
            ..BusConfig::new("Fone 2", BusKind::Monitor)
        },
    ];
    let push = |cfg: &mut MixerConfig, name: String, source: SourceMap, stereo: bool| {
        cfg.channels.push(ChannelConfig {
            name,
            source,
            fader_db: if stereo { -6.0 } else { OFF_DB },
            sends: vec![
                SendConfig { bus: 0, level_db: 0.0, ..Default::default() },
                SendConfig { bus: 1, enabled: stereo, level_db: OFF_DB, ..Default::default() },
                SendConfig { bus: 2, level_db: 0.0, ..Default::default() },
            ],
            ..Default::default()
        });
    };
    let mut k = 0;
    for p in io.inputs.iter().take(SECONDARY_BASE).filter(|p| p.present) {
        k += 1;
        push(&mut cfg, format!("Ch {k}"), SourceMap::Mono { input: p.index }, false);
    }
    let mut i = SECONDARY_BASE;
    while i < NET_BASE + 8 {
        let p = &io.inputs[i];
        let stereo = p.present && p.name.ends_with(" L") && io.inputs.get(i + 1).is_some_and(|n| n.present && n.name.ends_with(" R"));
        if stereo {
            push(&mut cfg, p.name.trim_end_matches(" L").to_string(), SourceMap::Stereo { left: i, right: i + 1 }, true);
            i += 2;
        } else {
            if p.present {
                push(&mut cfg, p.name.clone(), SourceMap::Mono { input: i }, false);
            }
            i += 1;
        }
    }
    cfg
}

pub use sona_core::level::OFF_DB;

fn primary_channel(p: &crate::audio::Port, n_buses: usize) -> ChannelConfig {
    ChannelConfig {
        name: format!("Ch {}", p.index + 1),
        source: SourceMap::Mono { input: p.index },
        fader_db: OFF_DB,
        sends: (0..n_buses.min(3))
            .map(|b| SendConfig { bus: b, enabled: b != 1, level_db: if b == 1 { OFF_DB } else { 0.0 }, ..Default::default() })
            .collect(),
        ..Default::default()
    }
}

/// Garante um canal para cada entrada do principal (ex.: a mesa foi escolhida
/// depois da primeira configuração, ou o driver tem entradas além das 32, como
/// o Player L/R da Ui24R). Entradas sem canal entram depois do último canal que
/// já usa o principal (ou no começo, se nenhum usa). Índices de solo e cenas
/// são deslocados juntos. `true` se mudou.
pub fn adopt_primary_inputs(mixer: &mut MixerConfig, scenes: &mut BTreeMap<String, MixerConfig>, io: &IoLayout) -> bool {
    use crate::audio::SECONDARY_BASE;
    let used: std::collections::HashSet<usize> = mixer.channels.iter().flat_map(|c| c.source.inputs()).collect();
    let missing: Vec<&crate::audio::Port> =
        io.inputs.iter().take(SECONDARY_BASE).filter(|p| p.present && !used.contains(&p.index)).collect();
    if missing.is_empty() {
        return false;
    }
    let at = mixer.channels.iter().rposition(|c| c.source.inputs().any(|i| i < SECONDARY_BASE)).map(|i| i + 1).unwrap_or(0);
    let new: Vec<ChannelConfig> = missing.iter().map(|p| primary_channel(p, mixer.buses.len())).collect();
    let n = new.len();
    let insert = |m: &mut MixerConfig| {
        let pos = at.min(m.channels.len());
        m.channels.splice(pos..pos, new.iter().cloned());
        for b in &mut m.buses {
            if let Some(mon) = &mut b.monitor {
                for c in &mut mon.solo {
                    if *c >= pos {
                        *c += n;
                    }
                }
            }
        }
    };
    insert(mixer);
    for sc in scenes.values_mut() {
        insert(sc);
    }
    true
}

/// Migra arquivos antigos para o formato atual.
pub fn migrate(mut v: Value) -> Result<Persisted, String> {
    let format = v.get("format").and_then(Value::as_u64).unwrap_or(0);
    if format == 0 {
        // Formato 0 (rascunho): o arquivo era só a MixerConfig.
        v = serde_json::json!({ "format": 1, "mixer": v, "scenes": {} });
    }
    if v["format"].as_u64() != Some(STATE_FORMAT as u64) {
        return Err(format!("Formato de estado {} é mais novo que este servidor.", v["format"]));
    }
    serde_json::from_value(v).map_err(|e| e.to_string())
}

/// Escrita atômica: arquivo temporário, fsync, rename.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// Prepara a configuração salva para o hardware atual. Canais que não existem
/// na lista atual ficam desligados no motor, mas continuam na configuração (a
/// UI mostra como desconectados). Aparelho presente mas fora do ar também gera
/// aviso (o motor já recebe silêncio dele). Vagas de rede sem bridge não avisam.
pub fn runtime_config(cfg: &MixerConfig, io: &IoLayout) -> (MixerConfig, Vec<plan::Issue>) {
    use crate::audio::NET_BASE;
    let mut rt = cfg.clone();
    let mut issues = Vec::new();
    let missing = |list: &[crate::audio::Port], i: usize| list.get(i).is_none_or(|p| !p.present);
    let offline = |list: &[crate::audio::Port], i: usize| list.get(i).is_some_and(|p| p.present && !p.connected && i < NET_BASE);
    for (i, ch) in rt.channels.iter_mut().enumerate() {
        if ch.source.inputs().any(|x| missing(&io.inputs, x)) {
            issues.push(plan::Issue {
                code: "input_disconnected",
                path: format!("/channels/{i}/source"),
                message: format!("A entrada de \"{}\" não existe na configuração de áudio atual.", ch.name),
            });
            ch.source = SourceMap::None;
        } else if ch.source.inputs().any(|x| offline(&io.inputs, x)) {
            issues.push(plan::Issue {
                code: "input_offline",
                path: format!("/channels/{i}/source"),
                message: format!("O aparelho da entrada de \"{}\" está fora do ar.", ch.name),
            });
        }
    }
    for (b, bus) in rt.buses.iter_mut().enumerate() {
        let before = bus.outputs.len();
        bus.outputs.retain(|o| !missing(&io.outputs, o.left) && !missing(&io.outputs, o.right));
        if bus.outputs.len() != before {
            issues.push(plan::Issue {
                code: "output_disconnected",
                path: format!("/buses/{b}/outputs"),
                message: format!("Uma saída de \"{}\" não existe na configuração de áudio atual.", bus.name),
            });
        }
        if bus.outputs.iter().any(|o| offline(&io.outputs, o.left)) {
            issues.push(plan::Issue {
                code: "output_offline",
                path: format!("/buses/{b}/outputs"),
                message: format!("O aparelho de saída de \"{}\" está fora do ar.", bus.name),
            });
        }
    }
    (rt, issues)
}

impl Store {
    pub fn load(path: PathBuf, io: IoLayout, layout: Layout) -> (Store, Engine) {
        let persisted = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Value>(&bytes).map_err(|e| e.to_string()).and_then(migrate) {
                Ok(p) => Some(p),
                Err(e) => {
                    let bad = path.with_extension(format!("corrupt-{}.json", std::process::id()));
                    tracing::error!("Estado ilegível ({e}). Guardei uma cópia em {} e comecei do padrão.", bad.display());
                    let _ = std::fs::copy(&path, &bad);
                    None
                }
            },
            Err(_) => None,
        };
        let fresh = persisted.is_none();
        let Persisted { mut mixer, mut scenes, .. } =
            persisted.unwrap_or_else(|| Persisted { format: STATE_FORMAT, mixer: default_mixer(&io), scenes: BTreeMap::new() });
        let adopted = adopt_primary_inputs(&mut mixer, &mut scenes, &io);
        let (rt, disconnected) = runtime_config(&mixer, &io);
        let (plan, mut validation) = match compile(&rt, &layout) {
            Ok(x) => x,
            Err(v) => {
                tracing::error!("Configuração salva inválida: {:?}. O motor começa vazio.", v.errors);
                (Plan::empty(), v)
            }
        };
        validation.warnings.extend(disconnected);
        let (engine, handle) = Engine::new(layout.clone(), plan);
        let mut store = Store { mixer, scenes, rev: 1, validation, io, layout, engine: handle, path, dirty: fresh || adopted };
        if fresh {
            store.save();
        }
        (store, engine)
    }

    pub fn meters(&self) -> std::sync::Arc<Meters> {
        self.engine.meters.clone()
    }

    /// Troca a lista de canais (nova configuração de áudio) e cria um motor novo
    /// com o mix atual. O motor antigo deve ter sido parado antes.
    pub fn rebuild(&mut self, io: IoLayout, layout: Layout) -> Engine {
        if adopt_primary_inputs(&mut self.mixer, &mut self.scenes, &io) {
            tracing::info!("Canais criados para as entradas do dispositivo principal.");
            self.dirty = true;
        }
        let (rt, disconnected) = runtime_config(&self.mixer, &io);
        let (plan, mut validation) = match compile(&rt, &layout) {
            Ok(x) => x,
            Err(v) => (Plan::empty(), v),
        };
        validation.warnings.extend(disconnected);
        let (engine, handle) = Engine::new(layout.clone(), plan);
        self.engine = handle;
        self.io = io;
        self.layout = layout;
        self.validation = validation;
        self.rev += 1;
        engine
    }

    /// Atualiza nome e estado de um canal de rede (bridge ligou ou saiu) sem
    /// mexer no motor.
    pub fn set_net_port(&mut self, slot: usize, name: Option<&str>) {
        for c in 0..2 {
            let idx = crate::audio::NET_BASE + 2 * slot + c;
            let side = if c == 0 { "L" } else { "R" };
            let label = match name {
                Some(n) => format!("{n} {side}"),
                None => format!("Rede {} {side}", slot + 1),
            };
            for list in [&mut self.io.inputs, &mut self.io.outputs] {
                if let Some(p) = list.get_mut(idx) {
                    p.name = label.clone();
                    p.connected = name.is_some();
                }
            }
        }
        self.rev += 1;
    }

    /// Valida, compila e entrega a nova configuração ao motor.
    pub fn commit(&mut self, next: MixerConfig) -> Result<(), PatchError> {
        let (rt, disconnected) = runtime_config(&next, &self.io);
        let (plan, mut v) = compile(&rt, &self.layout).map_err(PatchError::Rejected)?;
        if let Err(_p) = self.engine.submit(plan) {
            return Err(PatchError::Invalid("O motor de áudio não está recebendo mudanças.".into()));
        }
        v.warnings.extend(disconnected);
        self.mixer = next;
        self.validation = v;
        self.rev += 1;
        self.dirty = true;
        Ok(())
    }

    /// Aplica um JSON Merge Patch (RFC 7386) no caminho dado.
    pub fn patch(&mut self, path: &str, value: Value) -> Result<(), PatchError> {
        let mut doc = serde_json::to_value(&self.mixer).expect("serializa");
        let target = doc.pointer_mut(path).ok_or_else(|| PatchError::Invalid(format!("Caminho {path} não existe.")))?;
        merge(target, value);
        let next: MixerConfig = serde_json::from_value(doc).map_err(|e| PatchError::Invalid(format!("Valor inválido: {e}")))?;
        self.commit(next)
    }

    pub fn save_scene(&mut self, name: &str) {
        self.scenes.insert(name.to_string(), self.mixer.clone());
        self.rev += 1;
        self.dirty = true;
    }

    pub fn delete_scene(&mut self, name: &str) -> bool {
        let had = self.scenes.remove(name).is_some();
        if had {
            self.rev += 1;
            self.dirty = true;
        }
        had
    }

    /// Chama a cena sem mexer no roteamento físico: fontes dos canais, saídas,
    /// tipo e liga/desliga dos buses ficam como estão (são do admin).
    pub fn recall_scene(&mut self, name: &str) -> Result<(), PatchError> {
        let scene = self.scenes.get(name).cloned().ok_or_else(|| PatchError::Invalid(format!("Cena \"{name}\" não existe.")))?;
        let mut next = self.mixer.clone();
        for (cur, s) in next.channels.iter_mut().zip(scene.channels) {
            let source = cur.source;
            *cur = ChannelConfig { source, ..s };
        }
        let nb = next.buses.len();
        for (cur, s) in next.buses.iter_mut().zip(scene.buses) {
            let (outputs, enabled, kind) = (cur.outputs.clone(), cur.enabled, cur.kind);
            *cur = BusConfig { outputs, enabled, kind, ..s };
        }
        // Envios que apontam para buses que não existem mais caem fora.
        for ch in &mut next.channels {
            ch.sends.retain(|s| s.bus < nb);
        }
        self.commit(next)
    }

    pub fn snapshot(&self) -> Value {
        serde_json::json!({
            "rev": self.rev,
            "mixer": self.mixer,
            "scenes": self.scenes.keys().collect::<Vec<_>>(),
            "validation": self.validation,
            "io": self.io,
        })
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn save(&mut self) {
        let p = Persisted { format: STATE_FORMAT, mixer: self.mixer.clone(), scenes: self.scenes.clone() };
        match serde_json::to_vec_pretty(&p).map_err(std::io::Error::other).and_then(|b| write_atomic(&self.path, &b)) {
            Ok(()) => self.dirty = false,
            Err(e) => tracing::error!("Falha ao salvar {}: {e}", self.path.display()),
        }
        self.engine.collect();
    }

    pub fn collect(&mut self) {
        self.engine.collect();
    }
}

fn merge(target: &mut Value, patch: Value) {
    match patch {
        Value::Object(map) => {
            if !target.is_object() {
                *target = Value::Object(Default::default());
            }
            let t = target.as_object_mut().unwrap();
            for (k, v) in map {
                if v.is_null() {
                    t.remove(&k);
                } else {
                    merge(t.entry(k).or_insert(Value::Null), v);
                }
            }
        }
        other => *target = other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{config::AudioConfig, host::plan_io};

    #[test]
    fn mesa_escolhida_depois_ganha_canais_no_comeco() {
        // Primeira configuração sem mesa: só canais de rede.
        let (io0, ..) = plan_io(&AudioConfig { primary: "internal".into(), ..Default::default() });
        let mut mixer = default_mixer(&io0);
        assert!(mixer.channels.iter().all(|c| c.source.inputs().all(|i| i >= crate::audio::SECONDARY_BASE)));
        let rede = mixer.channels.len();
        mixer.buses[3].monitor.as_mut().unwrap().solo = vec![0];
        let mut scenes = BTreeMap::from([("Culto".to_string(), mixer.clone())]);
        // Depois a mesa (simulada, 24 entradas) vira o principal.
        let (io1, ..) = plan_io(&AudioConfig { primary: "sim".into(), ..Default::default() });
        assert!(adopt_primary_inputs(&mut mixer, &mut scenes, &io1));
        assert_eq!(mixer.channels.len(), rede + 24);
        assert_eq!(mixer.channels[23].name, "Ch 24");
        assert_eq!(mixer.channels[0].name, "Ch 1");
        assert_eq!(mixer.channels[0].source, SourceMap::Mono { input: 0 });
        assert_eq!(mixer.buses[3].monitor.as_ref().unwrap().solo, vec![24], "solo segue o mesmo canal");
        assert_eq!(scenes["Culto"].channels.len(), rede + 24, "cena deslocada junto");
        // Segunda vez não duplica.
        assert!(!adopt_primary_inputs(&mut mixer, &mut scenes, &io1));
    }
}

#[cfg(test)]
mod extra_tests {
    use super::*;
    use crate::audio::{config::AudioConfig, host::plan_io};

    /// Driver com entradas além das que já têm canal (ex.: Player L/R depois
    /// do 32): os canais novos entram logo depois do último canal da mesa.
    #[test]
    fn entradas_extras_entram_depois_dos_canais_da_mesa() {
        let (io, ..) = plan_io(&AudioConfig { primary: "sim".into(), ..Default::default() });
        let mut mixer = default_mixer(&io);
        // Simula uma instalação antiga que só criou 22 canais da mesa.
        mixer.channels.drain(22..24);
        let total = mixer.channels.len();
        mixer.buses[3].monitor.as_mut().unwrap().solo = vec![1, 22];
        let mut scenes = BTreeMap::new();
        assert!(adopt_primary_inputs(&mut mixer, &mut scenes, &io));
        assert_eq!(mixer.channels.len(), total + 2);
        assert_eq!(mixer.channels[22].name, "Ch 23");
        assert_eq!(mixer.channels[23].source, SourceMap::Mono { input: 23 });
        assert_eq!(mixer.buses[3].monitor.as_ref().unwrap().solo, vec![1, 24], "solo depois do ponto de inserção desloca");
        assert!(!adopt_primary_inputs(&mut mixer, &mut scenes, &io), "não duplica");
    }
}
