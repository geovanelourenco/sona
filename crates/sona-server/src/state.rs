//! Estado do mixer no servidor: configuração atual, cenas, revisão,
//! persistência atômica e entrega de planos ao motor.

use crate::audio::IoLayout;
use crate::auth::Role;
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
    Forbidden(String),
    Invalid(String),
    Rejected(Validation),
}

impl std::fmt::Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatchError::Forbidden(m) | PatchError::Invalid(m) => f.write_str(m),
            PatchError::Rejected(v) => {
                let msgs: Vec<_> = v.errors.iter().map(|i| i.message.as_str()).collect();
                write!(f, "Configuração recusada: {}", msgs.join(" "))
            }
        }
    }
}

/// Configuração inicial para o layout de demonstração (ver `audio::sim`).
pub fn default_mixer(io: &IoLayout) -> MixerConfig {
    let mut cfg = MixerConfig::default();
    let n_out = io.outputs.len();
    let out = |l: usize| (l + 1 < n_out).then(|| OutputPair { left: l, right: l + 1 }).into_iter().collect::<Vec<_>>();
    cfg.buses = vec![
        BusConfig {
            outputs: out(0),
            limiter: LimiterConfig { enabled: true, ceiling_db: -1.0, release_ms: 80.0 },
            ..BusConfig::new("Transmissão", BusKind::Broadcast)
        },
        BusConfig { outputs: out(2), ..BusConfig::new("Retorno PA", BusKind::PaReturn) },
        BusConfig { outputs: out(4), ..BusConfig::new("Gravação", BusKind::Recording) },
        BusConfig {
            outputs: out(6),
            monitor: Some(MonitorConfig { follow: Some(0), ..Default::default() }),
            ..BusConfig::new("Fone 1", BusKind::Monitor)
        },
        BusConfig {
            outputs: out(8),
            monitor: Some(MonitorConfig { follow: Some(0), ..Default::default() }),
            ..BusConfig::new("Fone 2", BusKind::Monitor)
        },
    ];
    let mut i = 0;
    let mut k = 0;
    while i < io.inputs.len() && cfg.channels.len() < 32 {
        let name = &io.inputs[i].name;
        let stereo_pair = name.ends_with(" L") && io.inputs.get(i + 1).is_some_and(|n| n.name.ends_with(" R"));
        let (source, label, step) = if stereo_pair {
            (SourceMap::Stereo { left: i, right: i + 1 }, name.trim_end_matches(" L").to_string(), 2)
        } else {
            k += 1;
            (SourceMap::Mono { input: i }, format!("Ch {k}"), 1)
        };
        cfg.channels.push(ChannelConfig {
            name: label,
            source,
            fader_db: if stereo_pair { -6.0 } else { OFF_DB },
            sends: vec![
                SendConfig { bus: 0, level_db: 0.0, ..Default::default() },
                SendConfig { bus: 1, enabled: stereo_pair, level_db: OFF_DB, ..Default::default() },
                SendConfig { bus: 2, level_db: 0.0, ..Default::default() },
            ],
            ..Default::default()
        });
        i += step;
    }
    cfg
}

pub use sona_core::level::OFF_DB;

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

/// Prepara a configuração salva para o hardware atual. Entradas e saídas que
/// sumiram ficam desligadas no motor, mas continuam na configuração (a UI
/// mostra como desconectadas). Devolve a cópia para o motor e os avisos.
pub fn runtime_config(cfg: &MixerConfig, layout: &Layout) -> (MixerConfig, Vec<plan::Issue>) {
    let mut rt = cfg.clone();
    let mut issues = Vec::new();
    for (i, ch) in rt.channels.iter_mut().enumerate() {
        if ch.source.inputs().any(|x| x >= layout.inputs) {
            issues.push(plan::Issue {
                code: "input_disconnected",
                path: format!("/channels/{i}/source"),
                message: format!("A entrada de \"{}\" não está conectada.", ch.name),
            });
            ch.source = SourceMap::None;
        }
    }
    for (b, bus) in rt.buses.iter_mut().enumerate() {
        let before = bus.outputs.len();
        bus.outputs.retain(|o| o.left < layout.outputs && o.right < layout.outputs);
        if bus.outputs.len() != before {
            issues.push(plan::Issue {
                code: "output_disconnected",
                path: format!("/buses/{b}/outputs"),
                message: format!("Uma saída de \"{}\" não está conectada.", bus.name),
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
        let Persisted { mixer, scenes, .. } =
            persisted.unwrap_or_else(|| Persisted { format: STATE_FORMAT, mixer: default_mixer(&io), scenes: BTreeMap::new() });
        let (rt, disconnected) = runtime_config(&mixer, &layout);
        let (plan, mut validation) = match compile(&rt, &layout) {
            Ok(x) => x,
            Err(v) => {
                tracing::error!("Configuração salva inválida: {:?}. O motor começa vazio.", v.errors);
                (Plan::empty(), v)
            }
        };
        validation.warnings.extend(disconnected);
        let (engine, handle) = Engine::new(layout.clone(), plan);
        let mut store = Store { mixer, scenes, rev: 1, validation, io, layout, engine: handle, path, dirty: fresh };
        if fresh {
            store.save();
        }
        (store, engine)
    }

    pub fn meters(&self) -> &std::sync::Arc<Meters> {
        &self.engine.meters
    }

    /// Valida, compila e entrega a nova configuração ao motor.
    pub fn commit(&mut self, next: MixerConfig) -> Result<(), PatchError> {
        let (rt, disconnected) = runtime_config(&next, &self.layout);
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
    pub fn patch(&mut self, role: Role, path: &str, value: Value) -> Result<(), PatchError> {
        authorize(role, path, &value, &self.mixer)?;
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

/// Campos que só o admin muda: roteamento físico e estrutura.
const ADMIN_CHANNEL_KEYS: &[&str] = &["source"];
const ADMIN_BUS_KEYS: &[&str] = &["outputs", "enabled", "kind", "bus_sends"];

fn touched_keys(path: &str, value: &Value) -> Vec<String> {
    // "/channels/3" + {"fader_db":..} → ["fader_db"]; "/channels/3/source" → ["source"].
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if parts.len() >= 3 {
        return vec![parts[2].to_string()];
    }
    value.as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default()
}

pub fn authorize(role: Role, path: &str, value: &Value, mixer: &MixerConfig) -> Result<(), PatchError> {
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let deny = |m: &str| Err(PatchError::Forbidden(m.into()));
    match role {
        Role::Admin => Ok(()),
        Role::Operator => match parts.as_slice() {
            ["channels", idx, ..] if idx.parse::<usize>().is_ok() => {
                if touched_keys(path, value).iter().any(|k| ADMIN_CHANNEL_KEYS.contains(&k.as_str())) {
                    deny("Só o admin muda a fonte de um canal.")
                } else {
                    Ok(())
                }
            }
            ["buses", idx, ..] if idx.parse::<usize>().is_ok() => {
                if touched_keys(path, value).iter().any(|k| ADMIN_BUS_KEYS.contains(&k.as_str())) {
                    deny("Só o admin muda saídas e ligação de buses.")
                } else {
                    Ok(())
                }
            }
            _ => deny("Só o admin muda a estrutura do mixer."),
        },
        Role::Monitor => match parts.as_slice() {
            ["buses", idx, rest @ ..] => {
                let Some(bus) = idx.parse::<usize>().ok().and_then(|b| mixer.buses.get(b)) else { return deny("Bus não existe.") };
                let only_monitor = match rest {
                    ["monitor", ..] => true,
                    [] => value.as_object().is_some_and(|m| m.keys().all(|k| k == "monitor")),
                    _ => false,
                };
                if bus.kind == BusKind::Monitor && only_monitor {
                    Ok(())
                } else {
                    deny("Perfil monitor só muda o próprio fone (solo e acompanhamento).")
                }
            }
            _ => deny("Perfil monitor não muda o mix."),
        },
    }
}
