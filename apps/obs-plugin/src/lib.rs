//! Plugin do OBS "Sona": fonte de áudio que recebe a transmissão do Sona
//! Bridge que roda neste computador (TCP 127.0.0.1:7360). Sem driver, sem cabo
//! virtual: o bridge recebe da rede, compensa a deriva e entrega aqui.
//!
//! O plugin não liga com o SDK do OBS. As poucas funções do libobs que usa são
//! procuradas em tempo de execução no módulo já carregado pelo OBS (obs.dll no
//! Windows, libobs.so no Linux). As estruturas abaixo espelham os cabeçalhos
//! do libobs 30 (`obs-source.h`, `obs.h`); `tests/obs/layout.c` confere os
//! tamanhos e deslocamentos contra os cabeçalhos oficiais.

use std::ffi::{c_char, c_void, CStr};
use std::io::Read;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

pub mod feed;

// ── Espelho do libobs 30 ─────────────────────────────────────────

pub const OBS_SOURCE_TYPE_INPUT: i32 = 0;
pub const OBS_SOURCE_AUDIO: u32 = 1 << 1;
pub const SPEAKERS_STEREO: i32 = 2;
pub const AUDIO_FORMAT_FLOAT_PLANAR: i32 = 8;
pub const MAX_AV_PLANES: usize = 8;
/// Versão da API para a qual o plugin foi escrito (30.0.0). OBS mais novo carrega.
pub const LIBOBS_API_VER: u32 = 30 << 24;

/// Início de `struct obs_source_info` (até `destroy`). O OBS aceita a
/// estrutura menor: `obs_register_source_s` recebe o tamanho.
#[repr(C)]
pub struct ObsSourceInfo {
    pub id: *const c_char,
    pub type_: i32,
    pub output_flags: u32,
    pub get_name: Option<unsafe extern "C" fn(*mut c_void) -> *const c_char>,
    pub create: Option<unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void>,
    pub destroy: Option<unsafe extern "C" fn(*mut c_void)>,
}

/// `struct obs_source_audio`.
#[repr(C)]
pub struct ObsSourceAudio {
    pub data: [*const u8; MAX_AV_PLANES],
    pub frames: u32,
    pub speakers: i32,
    pub format: i32,
    pub samples_per_sec: u32,
    pub timestamp: u64,
}

type RegisterSource = unsafe extern "C" fn(*const ObsSourceInfo, usize);
type OutputAudio = unsafe extern "C" fn(*mut c_void, *const ObsSourceAudio);
type GetTimeNs = unsafe extern "C" fn() -> u64;

struct Obs {
    output_audio: OutputAudio,
    gettime_ns: GetTimeNs,
}

static OBS: OnceLock<Obs> = OnceLock::new();

#[cfg(windows)]
fn libobs() -> Option<libloading::Library> {
    // SAFETY: obs.dll já está carregado no processo do OBS.
    unsafe { libloading::os::windows::Library::open_already_loaded("obs.dll").ok().map(Into::into) }
}

#[cfg(unix)]
fn libobs() -> Option<libloading::Library> {
    // Espaço de símbolos global do processo (onde o libobs já está).
    Some(libloading::os::unix::Library::this().into())
}

fn lookup<T: Copy>(lib: &libloading::Library, name: &[u8]) -> Option<T> {
    // SAFETY: os tipos espelham as assinaturas do libobs 30.
    unsafe { lib.get::<T>(name).ok().map(|s| *s) }
}

// ── Exportações exigidas pelo OBS ────────────────────────────────

static MODULE: std::sync::atomic::AtomicPtr<c_void> = std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

#[no_mangle]
pub extern "C" fn obs_module_set_pointer(module: *mut c_void) {
    MODULE.store(module, Relaxed);
}

#[no_mangle]
pub extern "C" fn obs_current_module() -> *mut c_void {
    MODULE.load(Relaxed)
}

#[no_mangle]
pub extern "C" fn obs_module_ver() -> u32 {
    LIBOBS_API_VER
}

#[no_mangle]
pub extern "C" fn obs_module_name() -> *const c_char {
    c"Sona".as_ptr()
}

#[no_mangle]
pub extern "C" fn obs_module_description() -> *const c_char {
    c"Recebe a transmissão do Sona Bridge (mesa de som do Lumoverso) como fonte de áudio.".as_ptr()
}

pub const SOURCE_ID: &CStr = c"sona_bridge_audio";

#[no_mangle]
pub extern "C" fn obs_module_load() -> bool {
    let Some(lib) = libobs() else { return false };
    let (Some(register), Some(output_audio), Some(gettime_ns)) = (
        lookup::<RegisterSource>(&lib, b"obs_register_source_s\0"),
        lookup::<OutputAudio>(&lib, b"obs_source_output_audio\0"),
        lookup::<GetTimeNs>(&lib, b"os_gettime_ns\0"),
    ) else {
        return false;
    };
    let _ = OBS.set(Obs { output_audio, gettime_ns });
    // O libobs continua carregado enquanto o OBS roda; não descarregar.
    std::mem::forget(lib);
    let info = ObsSourceInfo {
        id: SOURCE_ID.as_ptr(),
        type_: OBS_SOURCE_TYPE_INPUT,
        output_flags: OBS_SOURCE_AUDIO,
        get_name: Some(get_name),
        create: Some(create),
        destroy: Some(destroy),
    };
    // SAFETY: o OBS copia a estrutura (até o tamanho dado).
    unsafe { register(&info, std::mem::size_of::<ObsSourceInfo>()) };
    true
}

#[no_mangle]
pub extern "C" fn obs_module_unload() {}

// ── A fonte ──────────────────────────────────────────────────────

unsafe extern "C" fn get_name(_: *mut c_void) -> *const c_char {
    c"Sona (transmissão)".as_ptr()
}

struct Source {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// Ponteiro do OBS passado para a thread (o OBS garante a vida da fonte até `destroy`).
struct SourcePtr(*mut c_void);
// SAFETY: obs_source_output_audio é chamada de threads próprias por fontes
// assíncronas; o ponteiro vale até destroy, que espera a thread terminar.
unsafe impl Send for SourcePtr {}

unsafe extern "C" fn create(_settings: *mut c_void, source: *mut c_void) -> *mut c_void {
    let stop = Arc::new(AtomicBool::new(false));
    let (s2, ptr) = (stop.clone(), SourcePtr(source));
    let thread = std::thread::Builder::new().name("sona-obs".into()).spawn(move || run(ptr, s2)).ok();
    Box::into_raw(Box::new(Source { stop, thread })) as *mut c_void
}

unsafe extern "C" fn destroy(data: *mut c_void) {
    if data.is_null() {
        return;
    }
    // SAFETY: `data` veio de Box::into_raw em `create`.
    let mut s = unsafe { Box::from_raw(data as *mut Source) };
    s.stop.store(true, Relaxed);
    if let Some(t) = s.thread.take() {
        let _ = t.join();
    }
}

/// Conecta ao bridge, lê blocos e entrega ao OBS; reconecta a cada 1 s.
fn run(source: SourcePtr, stop: Arc<AtomicBool>) {
    let Some(obs) = OBS.get() else { return };
    while !stop.load(Relaxed) {
        let Ok(mut s) = TcpStream::connect_timeout(&feed::ADDR.parse().unwrap(), Duration::from_millis(500)) else {
            sleep_unless(&stop, Duration::from_secs(1));
            continue;
        };
        s.set_read_timeout(Some(Duration::from_millis(300))).ok();
        let Ok(h) = feed::read_header(&mut s) else { continue };
        let ch = h.channels as usize;
        let mut raw = vec![0u8; h.frames as usize * ch * 4];
        let mut planes = vec![vec![0.0f32; h.frames as usize]; ch];
        let chunk_ns = h.frames as u64 * 1_000_000_000 / h.sample_rate as u64;
        let mut next_ts = 0u64;
        while !stop.load(Relaxed) {
            match read_full(&mut s, &mut raw, &stop) {
                Ok(true) => {}
                _ => break,
            }
            feed::deinterleave(&raw, &mut planes);
            // SAFETY: função do libobs procurada em obs_module_load.
            let now = unsafe { (obs.gettime_ns)() };
            // Instantes contínuos pela contagem de amostras; realinha se escorregar.
            if next_ts == 0 || next_ts.abs_diff(now) > 60_000_000 {
                next_ts = now;
            }
            let mut data = [std::ptr::null::<u8>(); MAX_AV_PLANES];
            for (d, p) in data.iter_mut().zip(&planes) {
                *d = p.as_ptr() as *const u8;
            }
            let audio = ObsSourceAudio {
                data,
                frames: h.frames as u32,
                speakers: SPEAKERS_STEREO,
                format: AUDIO_FORMAT_FLOAT_PLANAR,
                samples_per_sec: h.sample_rate,
                timestamp: next_ts,
            };
            // SAFETY: a fonte vale até destroy (que espera esta thread).
            unsafe { (obs.output_audio)(source.0, &audio) };
            next_ts += chunk_ns;
        }
    }
}

fn read_full(s: &mut TcpStream, buf: &mut [u8], stop: &AtomicBool) -> std::io::Result<bool> {
    let mut got = 0;
    while got < buf.len() {
        if stop.load(Relaxed) {
            return Ok(false);
        }
        match s.read(&mut buf[got..]) {
            Ok(0) => return Ok(false),
            Ok(n) => got += n,
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

fn sleep_unless(stop: &AtomicBool, d: Duration) {
    let t0 = std::time::Instant::now();
    while t0.elapsed() < d && !stop.load(Relaxed) {
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    /// Mesmos números de tests/obs/layout.c (conferidos contra os cabeçalhos).
    #[test]
    fn estruturas_batem_com_o_libobs() {
        assert_eq!(offset_of!(ObsSourceInfo, type_), 8);
        assert_eq!(offset_of!(ObsSourceInfo, output_flags), 12);
        assert_eq!(offset_of!(ObsSourceInfo, get_name), 16);
        assert_eq!(offset_of!(ObsSourceInfo, create), 24);
        assert_eq!(offset_of!(ObsSourceInfo, destroy), 32);
        assert_eq!(size_of::<ObsSourceInfo>(), 40);
        assert_eq!(offset_of!(ObsSourceAudio, frames), 64);
        assert_eq!(offset_of!(ObsSourceAudio, speakers), 68);
        assert_eq!(offset_of!(ObsSourceAudio, format), 72);
        assert_eq!(offset_of!(ObsSourceAudio, samples_per_sec), 76);
        assert_eq!(offset_of!(ObsSourceAudio, timestamp), 80);
        assert_eq!(size_of::<ObsSourceAudio>(), 88);
    }
}
