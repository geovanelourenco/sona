//! Player multitrack: biblioteca de músicas (pasta `multitrack/` nos dados),
//! repertório, transporte e mix por faixa. O áudio é decodificado numa thread
//! de disco e entregue ao motor por uma fila sem trava (`audio::player_rt`).
//!
//! Saída: "simples" soma as faixas num canal estéreo "Player"; "faixa por
//! canal" põe cada faixa num canal da mesa. Click e guia (faixas `cue`) nunca
//! entram na soma: vão para o canal "Click + guia", que só toca nos fones.

pub mod decode;

use crate::audio::player_rt::{PlayerRt, PlayerShared, STRIDE};
use crate::audio::PLAYER_TRACKS;
use decode::TrackReader;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Fila entre disco e motor: 1 s.
const RING_SECONDS: usize = 1;
const CHUNK: usize = 1024;
pub const AUDIO_EXT: [&str; 6] = ["wav", "wave", "aif", "aiff", "flac", "mp3"];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct TrackMeta {
    /// Nome do arquivo na pasta da música.
    pub file: String,
    pub name: String,
    pub cue: bool,
    pub gain_db: f32,
    pub mute: bool,
    pub solo: bool,
    pub channels: u16,
    pub sample_rate: u32,
    pub frames: u64,
}

impl Default for TrackMeta {
    fn default() -> Self {
        TrackMeta {
            file: String::new(),
            name: String::new(),
            cue: false,
            gain_db: 0.0,
            mute: false,
            solo: false,
            channels: 2,
            sample_rate: 48_000,
            frames: 0,
        }
    }
}

impl TrackMeta {
    pub fn seconds(&self) -> f64 {
        self.frames as f64 / self.sample_rate.max(1) as f64
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Song {
    pub id: String,
    pub name: String,
    pub bpm: Option<f32>,
    pub key: String,
    pub tracks: Vec<TrackMeta>,
}

impl Song {
    pub fn seconds(&self) -> f64 {
        self.tracks.iter().map(|t| t.seconds()).fold(0.0, f64::max)
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Simple,
    Split,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Library {
    /// Ordem = repertório.
    pub songs: Vec<Song>,
    pub mode: Mode,
    pub master_db: f32,
    /// Para no fim de cada música (senão, toca a próxima).
    pub stop_at_end: bool,
}

impl Default for Library {
    fn default() -> Self {
        Library { songs: Vec::new(), mode: Mode::Simple, master_db: 0.0, stop_at_end: true }
    }
}

/// Click, guia e contagem vão para o fone por padrão.
pub fn looks_like_cue(name: &str) -> bool {
    let n = name.to_lowercase();
    ["click", "clique", "guia", "guide", "cue", "metronom", "metrônom", "count", "contagem"].iter().any(|k| n.contains(k))
}

/// "03_Bateria-L.wav" → "03 Bateria L".
pub fn track_name(file: &str) -> String {
    let stem = Path::new(file).file_stem().and_then(|s| s.to_str()).unwrap_or(file);
    stem.replace(['_', '-'], " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Nome de arquivo seguro (sem pastas, sem caracteres estranhos).
pub fn safe_file_name(name: &str) -> Option<String> {
    let base = name.rsplit(['/', '\\']).next()?.trim();
    let clean: String = base.chars().filter(|c| !c.is_control() && !matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|')).collect();
    let ext = Path::new(&clean).extension()?.to_str()?.to_lowercase();
    (!clean.starts_with('.') && clean.len() <= 120 && AUDIO_EXT.contains(&ext.as_str())).then_some(clean)
}

fn db_to_lin(db: f32) -> f32 {
    if db <= -60.0 {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

enum Cmd {
    Load(Vec<PathBuf>),
    Seek(u64),
    Attach(rtrb::Producer<f32>, u32),
    Quit,
}

pub struct Player {
    dir: PathBuf,
    pub lib: Mutex<Library>,
    pub sh: Arc<PlayerShared>,
    pub current: Mutex<Option<String>>,
    /// Faixas que falharam ao abrir na música atual.
    pub errors: Arc<Mutex<Vec<String>>>,
    sample_rate: std::sync::atomic::AtomicU32,
    tx: Mutex<mpsc::Sender<Cmd>>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Player {
    pub fn new(dir: PathBuf, sample_rate: u32) -> Arc<Player> {
        let lib: Library = std::fs::read(dir.join("library.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let sh = Arc::new(PlayerShared::default());
        let errors = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = mpsc::channel();
        let (sh2, err2) = (sh.clone(), errors.clone());
        let worker = std::thread::Builder::new()
            .name("sona-player-disco".into())
            .spawn(move || disk_worker(rx, sh2, err2, sample_rate))
            .expect("thread do player");
        let p = Arc::new(Player {
            dir,
            lib: Mutex::new(lib),
            sh,
            current: Mutex::new(None),
            errors,
            sample_rate: sample_rate.into(),
            tx: Mutex::new(tx),
            worker: Mutex::new(Some(worker)),
        });
        let first = p.lib.lock().unwrap().songs.first().map(|s| s.id.clone());
        if let Some(id) = first {
            let _ = p.load(&id);
        }
        p.apply_gains();
        p
    }

    /// Lado de tempo real para um motor novo (a fila antiga morre com ele).
    pub fn attach(&self, sample_rate: u32) -> PlayerRt {
        let (prod, cons) = rtrb::RingBuffer::new(sample_rate as usize * RING_SECONDS * STRIDE);
        self.sample_rate.store(sample_rate, Relaxed);
        let rt = PlayerRt::new(cons, self.sh.clone());
        let _ = self.tx.lock().unwrap().send(Cmd::Attach(prod, sample_rate));
        rt
    }

    pub fn shutdown(&self) {
        let _ = self.tx.lock().unwrap().send(Cmd::Quit);
        if let Some(t) = self.worker.lock().unwrap().take() {
            let _ = t.join();
        }
    }

    fn song_dir(&self, id: &str) -> PathBuf {
        self.dir.join(id)
    }

    pub fn save(&self) {
        let lib = self.lib.lock().unwrap().clone();
        let _ = std::fs::create_dir_all(&self.dir);
        if let Ok(b) = serde_json::to_vec_pretty(&lib) {
            let _ = crate::state::write_atomic(&self.dir.join("library.json"), &b);
        }
    }

    pub fn current_song(&self) -> Option<Song> {
        let id = self.current.lock().unwrap().clone()?;
        self.lib.lock().unwrap().songs.iter().find(|s| s.id == id).cloned()
    }

    /// Ganhos efetivos (volume, mute, solo, geral) e destino de cada faixa.
    pub fn apply_gains(&self) {
        let lib = self.lib.lock().unwrap();
        self.sh.split.store(lib.mode == Mode::Split, Relaxed);
        let cur = self.current.lock().unwrap().clone();
        let song = cur.and_then(|id| lib.songs.iter().find(|s| s.id == id));
        let tracks: &[TrackMeta] = song.map(|s| s.tracks.as_slice()).unwrap_or(&[]);
        let any_solo = tracks.iter().any(|t| t.solo);
        let master = db_to_lin(lib.master_db);
        for k in 0..PLAYER_TRACKS {
            let (g, cue) = match tracks.get(k) {
                Some(t) => {
                    let audible = !t.mute && (!any_solo || t.solo);
                    (if audible { db_to_lin(t.gain_db) * master } else { 0.0 }, t.cue)
                }
                None => (0.0, false),
            };
            self.sh.set_gain(k, g);
            self.sh.cue[k].store(cue, Relaxed);
        }
    }

    pub fn load(&self, id: &str) -> Result<(), String> {
        let song = self.lib.lock().unwrap().songs.iter().find(|s| s.id == id).cloned().ok_or("Música não encontrada.")?;
        self.sh.playing.store(false, Relaxed);
        let dir = self.song_dir(id);
        let paths = song.tracks.iter().take(PLAYER_TRACKS).map(|t| dir.join(&t.file)).collect();
        *self.current.lock().unwrap() = Some(id.to_string());
        self.apply_gains();
        let _ = self.tx.lock().unwrap().send(Cmd::Load(paths));
        Ok(())
    }

    pub fn play(&self) {
        if self.current_song().is_some_and(|s| !s.tracks.is_empty()) {
            self.sh.ended.store(false, Relaxed);
            self.sh.playing.store(true, Relaxed);
        }
    }

    pub fn pause(&self) {
        self.sh.playing.store(false, Relaxed);
    }

    pub fn seek_seconds(&self, s: f64) {
        let f = (s.max(0.0) * self.sample_rate.load(Relaxed) as f64) as u64;
        let _ = self.tx.lock().unwrap().send(Cmd::Seek(f));
    }

    pub fn stop(&self) {
        self.pause();
        self.seek_seconds(0.0);
    }

    /// Música vizinha no repertório (`delta` = +1 próxima, -1 anterior).
    pub fn neighbor(&self, delta: isize) -> Option<String> {
        let lib = self.lib.lock().unwrap();
        let cur = self.current.lock().unwrap().clone()?;
        let i = lib.songs.iter().position(|s| s.id == cur)? as isize + delta;
        (i >= 0).then(|| lib.songs.get(i as usize).map(|s| s.id.clone())).flatten()
    }

    pub fn create_song(&self, name: &str) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Dê um nome à música.".into());
        }
        let base: String = name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let base = if base.is_empty() { "musica".to_string() } else { base.chars().take(40).collect() };
        let mut lib = self.lib.lock().unwrap();
        let mut id = base.clone();
        let mut n = 2;
        while lib.songs.iter().any(|s| s.id == id) || self.song_dir(&id).exists() {
            id = format!("{base}-{n}");
            n += 1;
        }
        std::fs::create_dir_all(self.song_dir(&id)).map_err(|e| format!("Não consegui criar a pasta: {e}"))?;
        lib.songs.push(Song { id: id.clone(), name: name.chars().take(80).collect(), ..Default::default() });
        drop(lib);
        self.save();
        if self.current.lock().unwrap().is_none() {
            let _ = self.load(&id);
        }
        Ok(id)
    }

    /// Caminho onde gravar um arquivo enviado (antes de `add_file`).
    pub fn upload_path(&self, id: &str, file: &str) -> Result<PathBuf, String> {
        if !self.lib.lock().unwrap().songs.iter().any(|s| s.id == id) {
            return Err("Música não encontrada.".into());
        }
        let f = safe_file_name(file).ok_or("Arquivo não aceito. Use WAV, AIFF, FLAC ou MP3.")?;
        Ok(self.song_dir(id).join(f))
    }

    /// Lê o arquivo já gravado e o põe como faixa da música.
    pub fn add_file(&self, id: &str, file: &str) -> Result<TrackMeta, String> {
        let path = self.upload_path(id, file)?;
        let info = decode::probe(&path).inspect_err(|_| {
            let _ = std::fs::remove_file(&path);
        })?;
        let fname = path.file_name().and_then(|f| f.to_str()).unwrap_or(file).to_string();
        let name = track_name(&fname);
        let meta = TrackMeta {
            cue: looks_like_cue(&name),
            name,
            file: fname.clone(),
            channels: info.channels,
            sample_rate: info.sample_rate,
            frames: info.frames,
            ..Default::default()
        };
        {
            let mut lib = self.lib.lock().unwrap();
            let song = lib.songs.iter_mut().find(|s| s.id == id).ok_or("Música não encontrada.")?;
            if let Some(t) = song.tracks.iter_mut().find(|t| t.file == fname) {
                t.channels = info.channels;
                t.sample_rate = info.sample_rate;
                t.frames = info.frames;
            } else {
                if song.tracks.len() >= PLAYER_TRACKS {
                    return Err(format!("Limite de {PLAYER_TRACKS} faixas por música."));
                }
                song.tracks.push(meta.clone());
                song.tracks.sort_by_key(|t| t.file.to_lowercase());
            }
        }
        self.save();
        if self.current.lock().unwrap().as_deref() == Some(id) && !self.sh.playing.load(Relaxed) {
            let _ = self.load(id);
        }
        Ok(meta)
    }

    pub fn delete_song(&self, id: &str) -> Result<(), String> {
        {
            let mut lib = self.lib.lock().unwrap();
            let before = lib.songs.len();
            lib.songs.retain(|s| s.id != id);
            if lib.songs.len() == before {
                return Err("Música não encontrada.".into());
            }
        }
        if self.current.lock().unwrap().as_deref() == Some(id) {
            self.pause();
            *self.current.lock().unwrap() = None;
            let _ = self.tx.lock().unwrap().send(Cmd::Load(Vec::new()));
            let first = self.lib.lock().unwrap().songs.first().map(|s| s.id.clone());
            if let Some(f) = first {
                let _ = self.load(&f);
            }
        }
        let _ = std::fs::remove_dir_all(self.song_dir(id));
        self.save();
        Ok(())
    }

    pub fn remove_track(&self, id: &str, file: &str) -> Result<(), String> {
        {
            let mut lib = self.lib.lock().unwrap();
            let song = lib.songs.iter_mut().find(|s| s.id == id).ok_or("Música não encontrada.")?;
            song.tracks.retain(|t| t.file != file);
        }
        if let Some(f) = safe_file_name(file) {
            let _ = std::fs::remove_file(self.song_dir(id).join(f));
        }
        self.save();
        if self.current.lock().unwrap().as_deref() == Some(id) {
            let _ = self.load(id);
        }
        Ok(())
    }

    /// Muda nome, BPM, tom e mix das faixas. A ordem das faixas não muda aqui.
    pub fn update_song(&self, id: &str, patch: &Value) -> Result<(), String> {
        let mut lib = self.lib.lock().unwrap();
        let song = lib.songs.iter_mut().find(|s| s.id == id).ok_or("Música não encontrada.")?;
        if let Some(n) = patch["name"].as_str().map(str::trim).filter(|n| !n.is_empty()) {
            song.name = n.chars().take(80).collect();
        }
        if patch.get("bpm").is_some() {
            song.bpm = patch["bpm"].as_f64().map(|b| b as f32).filter(|b| (20.0..=400.0).contains(b));
        }
        if let Some(k) = patch["key"].as_str() {
            song.key = k.trim().chars().take(12).collect();
        }
        if let Some(list) = patch["tracks"].as_array() {
            for tp in list {
                let Some(file) = tp["file"].as_str() else { continue };
                let Some(t) = song.tracks.iter_mut().find(|t| t.file == file) else { continue };
                if let Some(n) = tp["name"].as_str().map(str::trim).filter(|n| !n.is_empty()) {
                    t.name = n.chars().take(40).collect();
                }
                if let Some(v) = tp["gain_db"].as_f64() {
                    t.gain_db = (v as f32).clamp(-60.0, 10.0);
                }
                if let Some(v) = tp["mute"].as_bool() {
                    t.mute = v;
                }
                if let Some(v) = tp["solo"].as_bool() {
                    t.solo = v;
                }
                if let Some(v) = tp["cue"].as_bool() {
                    t.cue = v;
                }
            }
        }
        drop(lib);
        self.apply_gains();
        self.save();
        Ok(())
    }

    pub fn reorder(&self, ids: &[String]) {
        let mut lib = self.lib.lock().unwrap();
        lib.songs.sort_by_key(|s| ids.iter().position(|i| *i == s.id).unwrap_or(usize::MAX));
        drop(lib);
        self.save();
    }

    pub fn set_settings(&self, v: &Value) {
        let mut lib = self.lib.lock().unwrap();
        if let Some(m) = v["mode"].as_str() {
            lib.mode = if m == "split" { Mode::Split } else { Mode::Simple };
        }
        if let Some(db) = v["master_db"].as_f64() {
            lib.master_db = (db as f32).clamp(-60.0, 10.0);
        }
        if let Some(b) = v["stop_at_end"].as_bool() {
            lib.stop_at_end = b;
        }
        drop(lib);
        self.apply_gains();
        self.save();
    }

    pub fn position_seconds(&self) -> f64 {
        self.sh.position.load(Relaxed) as f64 / self.sample_rate.load(Relaxed).max(1) as f64
    }

    /// Estado para a tela.
    pub fn snapshot(&self) -> Value {
        let lib = self.lib.lock().unwrap().clone();
        let current = self.current.lock().unwrap().clone();
        let songs: Vec<Value> = lib
            .songs
            .iter()
            .map(|s| json!({ "id": s.id, "name": s.name, "bpm": s.bpm, "key": s.key, "seconds": s.seconds(), "tracks": s.tracks }))
            .collect();
        json!({
            "songs": songs,
            "mode": lib.mode,
            "master_db": lib.master_db,
            "stop_at_end": lib.stop_at_end,
            "current": current,
            "playing": self.sh.playing.load(Relaxed),
            "position": self.position_seconds(),
            "underruns": self.sh.underruns.load(Relaxed),
            "errors": self.errors.lock().unwrap().clone(),
        })
    }

    /// Posição e níveis por faixa (enviado a ~10 Hz).
    pub fn tick(&self) -> Value {
        let n = self.current_song().map(|s| s.tracks.len()).unwrap_or(0).min(PLAYER_TRACKS);
        let peaks: Vec<f64> = (0..n).map(|k| ((self.sh.take_peak(k) as f64) * 1000.0).round() / 1000.0).collect();
        json!({ "type": "player.tick", "position": self.position_seconds(), "playing": self.sh.playing.load(Relaxed), "peaks": peaks })
    }

    /// Canais que a mesa precisa para o modo atual: (nome, porta L, estéreo).
    pub fn channel_spec(&self) -> Vec<(String, usize, bool)> {
        use crate::audio::{PLAYER_CUE, PLAYER_SUM, PLAYER_TRACK_BASE};
        let lib = self.lib.lock().unwrap();
        if lib.songs.is_empty() {
            return Vec::new();
        }
        let mut v = vec![("Click + guia".to_string(), PLAYER_CUE, true)];
        match lib.mode {
            Mode::Simple => v.insert(0, ("Player".to_string(), PLAYER_SUM, true)),
            Mode::Split => {
                let cur = self.current.lock().unwrap().clone();
                if let Some(song) = cur.and_then(|id| lib.songs.iter().find(|s| s.id == id)) {
                    for (k, t) in song.tracks.iter().enumerate().take(PLAYER_TRACKS) {
                        if !t.cue {
                            v.push((format!("MT {}", t.name), PLAYER_TRACK_BASE + 2 * k, t.channels > 1));
                        }
                    }
                }
            }
        }
        v
    }
}

fn disk_worker(rx: mpsc::Receiver<Cmd>, sh: Arc<PlayerShared>, errors: Arc<Mutex<Vec<String>>>, mut rate: u32) {
    let mut prod: Option<rtrb::Producer<f32>> = None;
    let mut paths: Vec<PathBuf> = Vec::new();
    let mut readers: Vec<Option<TrackReader>> = Vec::new();
    let mut written: u64 = 0;
    let mut total: u64 = 0;
    let mut tmp = vec![vec![0.0f32; 2 * CHUNK]; PLAYER_TRACKS];
    let mut frame_buf = vec![0.0f32; STRIDE * CHUNK];

    // Esvazia a fila no motor e espera ele confirmar (no máximo 300 ms: sem
    // motor rodando, ninguém confirma e a fila já não importa).
    let flush = |sh: &PlayerShared, to: u64| {
        sh.ready.store(false, Release);
        sh.seek_to.store(to, Relaxed);
        sh.eof_at.store(u64::MAX, Release);
        let req = sh.flush_req.fetch_add(1, Release) + 1;
        let t0 = Instant::now();
        while sh.flush_ack.load(Acquire) < req && t0.elapsed() < Duration::from_millis(300) {
            std::thread::sleep(Duration::from_millis(2));
        }
        if sh.flush_ack.load(Acquire) < req {
            sh.position.store(to, Relaxed);
        }
    };
    let open_all = |paths: &[PathBuf], rate: u32, errors: &Mutex<Vec<String>>| -> (Vec<Option<TrackReader>>, u64) {
        let mut errs = Vec::new();
        let rs: Vec<Option<TrackReader>> = paths
            .iter()
            .map(|p| match TrackReader::open(p, rate) {
                Ok(r) => Some(r),
                Err(e) => {
                    errs.push(format!("{}: {e}", p.file_name().and_then(|f| f.to_str()).unwrap_or("?")));
                    None
                }
            })
            .collect();
        let total = rs.iter().flatten().map(|r| r.out_frames()).max().unwrap_or(0);
        *errors.lock().unwrap() = errs;
        (rs, total)
    };

    loop {
        let full = prod.as_ref().is_none_or(|p| p.slots() < STRIDE * CHUNK) || written >= total;
        let cmd = if full { rx.recv_timeout(Duration::from_millis(5)) } else { rx.try_recv().map_err(|_| mpsc::RecvTimeoutError::Timeout) };
        match cmd {
            Ok(Cmd::Quit) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(Cmd::Load(p)) => {
                flush(&sh, 0);
                paths = p;
                let (rs, t) = open_all(&paths, rate, &errors);
                readers = rs;
                total = t;
                written = 0;
            }
            Ok(Cmd::Seek(f)) => {
                let f = f.min(total);
                flush(&sh, f);
                for r in readers.iter_mut().flatten() {
                    r.seek(f);
                }
                written = f;
            }
            Ok(Cmd::Attach(p, sr)) => {
                let pos = sh.position.load(Relaxed);
                if sr != rate {
                    // Taxa nova: reabre com a conversão certa, no mesmo instante.
                    let secs = pos as f64 / rate as f64;
                    rate = sr;
                    let (rs, t) = open_all(&paths, rate, &errors);
                    readers = rs;
                    total = t;
                    let f = (secs * rate as f64) as u64;
                    sh.position.store(f, Relaxed);
                }
                let pos = sh.position.load(Relaxed).min(total);
                for r in readers.iter_mut().flatten() {
                    r.seek(pos);
                }
                written = pos;
                sh.ready.store(false, Release);
                sh.eof_at.store(u64::MAX, Release);
                prod = Some(p);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if written >= total {
            // Nada mais a ler (música vazia, ou já no fim): o motor para sozinho.
            sh.eof_at.store(total, Release);
        }
        // Enche a fila.
        let Some(p) = prod.as_mut() else { continue };
        while written < total && p.slots() >= STRIDE * CHUNK {
            let n = CHUNK.min((total - written) as usize);
            for (k, buf) in tmp.iter_mut().enumerate() {
                match readers.get_mut(k).and_then(|r| r.as_mut()) {
                    Some(r) => {
                        r.read(&mut buf[..2 * n]);
                    }
                    None => buf[..2 * n].fill(0.0),
                }
            }
            for i in 0..n {
                for k in 0..PLAYER_TRACKS {
                    frame_buf[i * STRIDE + 2 * k] = tmp[k][2 * i];
                    frame_buf[i * STRIDE + 2 * k + 1] = tmp[k][2 * i + 1];
                }
            }
            if let Ok(mut chunk) = p.write_chunk_uninit(n * STRIDE) {
                let (a, b) = chunk.as_mut_slices();
                for (dst, src) in a.iter_mut().chain(b.iter_mut()).zip(&frame_buf[..n * STRIDE]) {
                    dst.write(*src);
                }
                // SAFETY: todos os elementos foram escritos acima.
                unsafe { chunk.commit_all() };
            }
            written += n as u64;
            sh.ready.store(true, Release);
            if written >= total {
                sh.eof_at.store(total, Release);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{PLAYER_CUE, PLAYER_SUM, PORTS};

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sona-player-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn nomes_e_click() {
        assert_eq!(track_name("03_Bateria-L.wav"), "03 Bateria L");
        assert!(looks_like_cue("Click"));
        assert!(looks_like_cue("Guia de voz"));
        assert!(!looks_like_cue("Baixo"));
        assert_eq!(safe_file_name("../x/Baixo.WAV").as_deref(), Some("Baixo.WAV"));
        assert!(safe_file_name("notas.txt").is_none());
        assert!(safe_file_name(".wav").is_none());
    }

    /// Fluxo completo sem motor real: cria a música, envia dois arquivos, toca
    /// e confere que a soma e o fone recebem áudio pelo lado de tempo real.
    #[test]
    fn toca_soma_e_fone() {
        let d = dir("fluxo");
        let p = Player::new(d.clone(), 48_000);
        let mut rt = p.attach(48_000);
        let id = p.create_song("Louvor 2").unwrap();
        assert_eq!(id, "louvor-2");
        for (f, freq) in [("Baixo.wav", 110.0), ("Click.wav", 1000.0)] {
            let path = p.upload_path(&id, f).unwrap();
            decode::tests::write_wav(&path, 44_100, 1, 0.5, freq);
            p.add_file(&id, f).unwrap();
        }
        let song = p.current_song().unwrap();
        assert_eq!(song.tracks.len(), 2);
        assert!(song.tracks[1].cue, "click vai para o fone");
        assert_eq!(p.channel_spec().len(), 2);
        // Espera o disco encher a fila.
        let t0 = Instant::now();
        let mut ins = vec![vec![0.0f32; 256]; PORTS];
        p.play();
        let (mut sum, mut cue) = (0.0f32, 0.0f32);
        while t0.elapsed() < Duration::from_secs(5) && p.sh.position.load(Relaxed) < 20_000 {
            rt.render(&mut ins, 256);
            sum = sum.max(ins[PLAYER_SUM][100].abs());
            cue = cue.max(ins[PLAYER_CUE][100].abs());
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(sum > 0.05 && cue > 0.05, "soma {sum}, fone {cue}");
        // Mute do baixo zera a soma.
        p.update_song(&id, &json!({"tracks": [{"file": "Baixo.wav", "mute": true}]})).unwrap();
        rt.render(&mut ins, 256);
        rt.render(&mut ins, 256);
        assert_eq!(ins[PLAYER_SUM][100], 0.0);
        // Biblioteca persiste.
        p.shutdown();
        let p2 = Player::new(d.clone(), 48_000);
        assert!(p2.lib.lock().unwrap().songs[0].tracks[0].mute);
        assert_eq!(p2.current.lock().unwrap().as_deref(), Some("louvor-2"));
        p2.shutdown();
        let _ = std::fs::remove_dir_all(d);
    }
}
