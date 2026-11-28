//! Aparelhos do bridge: um para enviar (entrada do sistema → servidor) e um
//! para receber (servidor → saída do sistema). "sim" cria aparelhos simulados:
//! um gerador de tom (liga 0,5 s, desliga 0,5 s) e um medidor que calcula o
//! atraso de ida e volta comparando o início de cada tom.

use sona_core::drift::Reader;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const SIM: &str = "sim";
/// Recepção para o plugin "Sona" do OBS neste computador (127.0.0.1:7360).
pub const OBS: &str = "obs";
/// Prefixo de envio que captura o que toca numa saída do sistema (loopback do
/// WASAPI no Windows): "loopback:Alto-falantes (Realtek…)". Sem cabo virtual.
pub const LOOPBACK: &str = "loopback:";

/// Níveis e medições para a tela (sem trava).
#[derive(Default)]
pub struct Levels {
    /// Pico (bits de f32) desde a última leitura.
    pub send_peak: AtomicU32,
    pub recv_peak: AtomicU32,
    /// Medidor simulado: último atraso de ida e volta (µs) e quantas medições.
    pub sim_latency_us: AtomicU64,
    pub sim_measurements: AtomicU64,
    /// Inícios de tom do gerador simulado, para o medidor casar.
    pub onsets: Mutex<VecDeque<Instant>>,
}

impl Levels {
    pub fn put(slot: &AtomicU32, v: f32) {
        if v.is_finite() {
            slot.fetch_max(v.abs().to_bits(), Relaxed);
        }
    }
    pub fn take(slot: &AtomicU32) -> f32 {
        f32::from_bits(slot.swap(0, Relaxed))
    }
}

/// Cópia do áudio para a escuta ("ouvir o que chega"): quem toca ou captura
/// empurra aqui sem bloquear; a saída de escuta lê com deriva compensada.
/// Mudar a escuta é raro, então `try_lock` no callback nunca espera.
pub struct Tap {
    inner: Mutex<TapInner>,
    /// Volume da escuta (bits de f32, linear).
    pub gain: Arc<AtomicU32>,
}

struct TapInner {
    writer: sona_core::drift::Writer,
    scratch: Vec<f32>,
}

pub type TapRef = Option<Arc<Tap>>;

impl Tap {
    pub fn new(writer: sona_core::drift::Writer, gain: Arc<AtomicU32>) -> Arc<Tap> {
        Arc::new(Tap { inner: Mutex::new(TapInner { writer, scratch: vec![0.0; 4096] }), gain })
    }

    pub fn push_planar(&self, l: &[f32], r: &[f32]) {
        let Ok(mut t) = self.inner.try_lock() else { return };
        let g = f32::from_bits(self.gain.load(Relaxed));
        let TapInner { writer, scratch } = &mut *t;
        let n = l.len().min(r.len());
        let mut i = 0;
        while i < n {
            let k = (n - i).min(scratch.len() / 2);
            for j in 0..k {
                scratch[2 * j] = l[i + j] * g;
                scratch[2 * j + 1] = r[i + j] * g;
            }
            writer.push_interleaved(&scratch[..2 * k]);
            i += k;
        }
    }

    pub fn push_interleaved(&self, data: &[f32]) {
        let Ok(mut t) = self.inner.try_lock() else { return };
        let g = f32::from_bits(self.gain.load(Relaxed));
        let TapInner { writer, scratch } = &mut *t;
        for c in data.chunks(scratch.len()) {
            for (d, s) in scratch.iter_mut().zip(c) {
                *d = s * g;
            }
            writer.push_interleaved(&scratch[..c.len()]);
        }
    }
}

pub fn db_to_gain(db: f32) -> f32 {
    if db <= -60.0 {
        0.0
    } else {
        10f32.powf(db / 20.0)
    }
}

pub enum Handle {
    Thread(Arc<AtomicBool>, Option<std::thread::JoinHandle<()>>),
    #[cfg(feature = "hw")]
    Stream(#[allow(dead_code)] cpal::Stream),
}

impl Drop for Handle {
    fn drop(&mut self) {
        if let Handle::Thread(stop, t) = self {
            stop.store(true, Relaxed);
            if let Some(t) = t.take() {
                let _ = t.join();
            }
        }
    }
}

/// Liga o aparelho de envio: escreve áudio estéreo intercalado em `out`.
pub fn start_send(
    device: &str,
    sample_rate: u32,
    ppm: f64,
    mut out: rtrb::Producer<f32>,
    levels: Arc<Levels>,
    tap: TapRef,
) -> Result<Handle, String> {
    if device == SIM {
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        let t = std::thread::Builder::new()
            .name("bridge-sim-envio".into())
            .spawn(move || {
                let rate = sample_rate as f64 * (1.0 + ppm * 1e-6);
                let block = 120usize;
                let period = Duration::from_secs_f64(block as f64 / rate);
                let (mut n, mut phase) = (0u64, 0.0f64);
                let mut deadline = Instant::now();
                let mut buf = vec![0.0f32; block * 2];
                while !s2.load(Relaxed) {
                    for i in 0..block {
                        let on = (n / (sample_rate as u64 / 2)).is_multiple_of(2);
                        if on && n % (sample_rate as u64 / 2) == 0 {
                            let mut q = levels.onsets.lock().unwrap();
                            q.push_back(Instant::now() + Duration::from_secs_f64(i as f64 / rate));
                            if q.len() > 16 {
                                q.pop_front();
                            }
                        }
                        let v = if on { (0.5 * phase.sin()) as f32 } else { 0.0 };
                        phase = (phase + std::f64::consts::TAU * 1000.0 / rate) % std::f64::consts::TAU;
                        buf[2 * i] = v;
                        buf[2 * i + 1] = v;
                        n += 1;
                    }
                    Levels::put(&levels.send_peak, 0.5);
                    if let Some(t) = &tap {
                        t.push_interleaved(&buf);
                    }
                    for s in &buf {
                        let _ = out.push(*s);
                    }
                    deadline += period;
                    let now = Instant::now();
                    if deadline > now {
                        std::thread::sleep(deadline - now);
                    } else if now - deadline > Duration::from_millis(200) {
                        deadline = now;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        return Ok(Handle::Thread(stop, Some(t)));
    }
    #[cfg(feature = "hw")]
    {
        hw::send(device, sample_rate, out, levels, tap)
    }
    #[cfg(not(feature = "hw"))]
    {
        let _ = tap;
        Err(format!("\"{device}\": compilado sem suporte a aparelhos reais."))
    }
}

/// Liga o aparelho de recepção: lê estéreo de `reader` (com deriva compensada).
pub fn start_receive(
    device: &str,
    sample_rate: u32,
    ppm: f64,
    mut reader: Reader,
    levels: Arc<Levels>,
    tap: TapRef,
) -> Result<Handle, String> {
    if device == OBS {
        return start_obs_feed(sample_rate, reader, levels, tap);
    }
    if device.is_empty() {
        return start_null_sink(sample_rate, reader, levels, tap);
    }
    if device == SIM {
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        let t = std::thread::Builder::new()
            .name("bridge-sim-medidor".into())
            .spawn(move || {
                // Relógio do medidor com metade do desvio, para ser outro relógio.
                let rate = sample_rate as f64 * (1.0 - ppm * 0.5e-6);
                let block = 256usize;
                let period = Duration::from_secs_f64(block as f64 / rate);
                let (mut l, mut r) = (vec![0.0f32; block], vec![0.0f32; block]);
                let mut deadline = Instant::now();
                let mut quiet = 0usize;
                while !s2.load(Relaxed) {
                    {
                        let mut outs: [&mut [f32]; 2] = [&mut l, &mut r];
                        reader.pull_planar(&mut outs, block);
                    }
                    if let Some(t) = &tap {
                        t.push_planar(&l, &r);
                    }
                    for (i, v) in l.iter().enumerate() {
                        Levels::put(&levels.recv_peak, *v);
                        if v.abs() > 0.02 {
                            // Início de tom depois de 0,2 s de silêncio: casa com o envio.
                            if quiet > sample_rate as usize / 5 {
                                let heard = Instant::now() + Duration::from_secs_f64(i as f64 / rate);
                                let mut q = levels.onsets.lock().unwrap();
                                while let Some(&t) = q.front() {
                                    if t > heard {
                                        break;
                                    }
                                    q.pop_front();
                                    let d = heard - t;
                                    if d < Duration::from_millis(400) {
                                        levels.sim_latency_us.store(d.as_micros() as u64, Relaxed);
                                        levels.sim_measurements.fetch_add(1, Relaxed);
                                    }
                                }
                            }
                            quiet = 0;
                        } else {
                            quiet += 1;
                        }
                    }
                    deadline += period;
                    let now = Instant::now();
                    if deadline > now {
                        std::thread::sleep(deadline - now);
                    } else if now - deadline > Duration::from_millis(200) {
                        deadline = now;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        return Ok(Handle::Thread(stop, Some(t)));
    }
    #[cfg(feature = "hw")]
    {
        hw::receive(device, sample_rate, reader, levels, tap)
    }
    #[cfg(not(feature = "hw"))]
    {
        let _ = tap;
        Err(format!("\"{device}\": compilado sem suporte a aparelhos reais."))
    }
}

/// Recebe sem tocar em lugar nenhum (só para a escuta), no ritmo deste computador.
fn start_null_sink(sample_rate: u32, mut reader: Reader, levels: Arc<Levels>, tap: TapRef) -> Result<Handle, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = stop.clone();
    let t = std::thread::Builder::new()
        .name("bridge-escuta".into())
        .spawn(move || {
            const FRAMES: usize = 480;
            let period = Duration::from_secs_f64(FRAMES as f64 / sample_rate as f64);
            let (mut l, mut r) = (vec![0.0f32; FRAMES], vec![0.0f32; FRAMES]);
            let mut deadline = Instant::now();
            while !s2.load(Relaxed) {
                {
                    let mut outs: [&mut [f32]; 2] = [&mut l, &mut r];
                    reader.pull_planar(&mut outs, FRAMES);
                }
                Levels::put(&levels.recv_peak, l.iter().chain(r.iter()).fold(0.0f32, |m, v| m.max(v.abs())));
                if let Some(t) = &tap {
                    t.push_planar(&l, &r);
                }
                deadline += period;
                let now = Instant::now();
                if deadline > now {
                    std::thread::sleep(deadline - now);
                } else if now - deadline > Duration::from_millis(200) {
                    deadline = now;
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(Handle::Thread(stop, Some(t)))
}

#[cfg(feature = "hw")]
pub mod hw {
    use super::{Handle, Levels, TapRef};
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
    use serde::Serialize;
    use sona_core::drift::Reader;
    use std::sync::Arc;

    #[derive(Serialize)]
    pub struct Dev {
        pub name: String,
        pub host: String,
        pub inputs: u16,
        pub outputs: u16,
    }

    fn name_of(d: &cpal::Device) -> String {
        d.description().map(|x| x.name().to_string()).unwrap_or_else(|_| d.to_string())
    }

    fn max_ch<I: Iterator<Item = cpal::SupportedStreamConfigRange>>(it: Result<I, cpal::Error>, sr: u32) -> u16 {
        it.map(|i| i.filter(|r| r.min_sample_rate() <= sr && r.max_sample_rate() >= sr).map(|r| r.channels()).max().unwrap_or(0))
            .unwrap_or(0)
    }

    pub fn list(sr: u32) -> Vec<Dev> {
        let mut out = Vec::new();
        for id in cpal::available_hosts() {
            let Ok(host) = cpal::host_from_id(id) else { continue };
            let Ok(devs) = host.devices() else { continue };
            for d in devs {
                out.push(Dev {
                    name: name_of(&d),
                    host: id.name().into(),
                    inputs: max_ch(d.supported_input_configs(), sr),
                    outputs: max_ch(d.supported_output_configs(), sr),
                });
            }
        }
        out
    }

    /// Acha o aparelho pelo nome; `output` exige que tenha saída (loopback).
    fn find_with(name: &str, output: bool) -> Option<cpal::Device> {
        for id in cpal::available_hosts() {
            let Ok(host) = cpal::host_from_id(id) else { continue };
            let Ok(devs) = host.devices() else { continue };
            for d in devs {
                let n = name_of(&d);
                if (n == name || n.contains(name)) && (!output || d.supports_output()) {
                    return Some(d);
                }
            }
        }
        None
    }

    fn find(name: &str) -> Option<cpal::Device> {
        find_with(name, false)
    }

    fn pick(d: &cpal::Device, input: bool, sr: u32) -> Option<(u16, SampleFormat)> {
        let list: Vec<_> = if input { d.supported_input_configs().ok()?.collect() } else { d.supported_output_configs().ok()?.collect() };
        let ok: Vec<_> = list.iter().filter(|r| r.min_sample_rate() <= sr && r.max_sample_rate() >= sr).collect();
        let ch = if ok.iter().any(|r| r.channels() == 2) { 2 } else { ok.iter().map(|r| r.channels()).min()? };
        let fmt = [SampleFormat::F32, SampleFormat::I32, SampleFormat::I24, SampleFormat::I16]
            .into_iter()
            .find(|f| ok.iter().any(|r| r.channels() == ch && r.sample_format() == *f))?;
        Some((ch, fmt))
    }

    pub fn send(device: &str, sr: u32, out: rtrb::Producer<f32>, levels: Arc<Levels>, tap: TapRef) -> Result<Handle, String> {
        if let Some(name) = device.strip_prefix(super::LOOPBACK) {
            return loopback(name, sr, out, levels, tap);
        }
        let d = find(device).ok_or_else(|| format!("\"{device}\" não encontrado."))?;
        let (ch, fmt) = pick(&d, true, sr).ok_or_else(|| format!("\"{device}\" não tem entrada em {sr} Hz."))?;
        let cfg = StreamConfig { channels: ch, sample_rate: sr, buffer_size: cpal::BufferSize::Default };
        let s = match fmt {
            SampleFormat::F32 => input::<f32>(&d, cfg, out, levels, tap),
            SampleFormat::I32 => input::<i32>(&d, cfg, out, levels, tap),
            SampleFormat::I24 => input::<cpal::I24>(&d, cfg, out, levels, tap),
            _ => input::<i16>(&d, cfg, out, levels, tap),
        }?;
        s.play().map_err(|e| e.to_string())?;
        Ok(Handle::Stream(s))
    }

    /// Captura o que toca numa saída (WASAPI abre a saída como entrada em modo
    /// loopback). O formato é o da saída; a taxa pedida é a da sessão (48 kHz).
    fn loopback(name: &str, sr: u32, out: rtrb::Producer<f32>, levels: Arc<Levels>, tap: TapRef) -> Result<Handle, String> {
        let d = find_with(name, true).ok_or_else(|| format!("Saída \"{name}\" não encontrada."))?;
        let (ch, fmt) = pick(&d, false, sr)
            .ok_or_else(|| format!("\"{name}\" não aceita {sr} Hz. Ajuste a saída para 48 kHz nas configurações de som do Windows."))?;
        let cfg = StreamConfig { channels: ch, sample_rate: sr, buffer_size: cpal::BufferSize::Default };
        let s = match fmt {
            SampleFormat::F32 => input::<f32>(&d, cfg, out, levels, tap),
            SampleFormat::I32 => input::<i32>(&d, cfg, out, levels, tap),
            SampleFormat::I24 => input::<cpal::I24>(&d, cfg, out, levels, tap),
            _ => input::<i16>(&d, cfg, out, levels, tap),
        }
        .map_err(|e| {
            if cfg!(windows) {
                format!("Não consegui capturar o áudio de \"{name}\": {e}")
            } else {
                format!("Captura do áudio do sistema só funciona no Windows ({e}).")
            }
        })?;
        s.play().map_err(|e| e.to_string())?;
        Ok(Handle::Stream(s))
    }

    fn input<T>(
        d: &cpal::Device,
        cfg: StreamConfig,
        mut out: rtrb::Producer<f32>,
        levels: Arc<Levels>,
        tap: TapRef,
    ) -> Result<cpal::Stream, String>
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
        let ch = cfg.channels as usize;
        let mut copy = [0.0f32; 512];
        d.build_input_stream::<T, _, _>(
            cfg,
            move |data: &[T], _| {
                let mut peak = 0.0f32;
                let mut k = 0;
                for f in data.chunks(ch) {
                    let l = <f32 as FromSample<T>>::from_sample_(f[0]);
                    let r = if ch > 1 { <f32 as FromSample<T>>::from_sample_(f[1]) } else { l };
                    peak = peak.max(l.abs()).max(r.abs());
                    let _ = out.push(l);
                    let _ = out.push(r);
                    if let Some(t) = &tap {
                        copy[k] = l;
                        copy[k + 1] = r;
                        k += 2;
                        if k == copy.len() {
                            t.push_interleaved(&copy);
                            k = 0;
                        }
                    }
                }
                if let (Some(t), true) = (&tap, k > 0) {
                    t.push_interleaved(&copy[..k]);
                }
                Levels::put(&levels.send_peak, peak);
            },
            |e| tracing::warn!("entrada: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
    }

    pub fn receive(device: &str, sr: u32, reader: Reader, levels: Arc<Levels>, tap: TapRef) -> Result<Handle, String> {
        let d = find(device).ok_or_else(|| format!("\"{device}\" não encontrado."))?;
        let (ch, fmt) = pick(&d, false, sr).ok_or_else(|| format!("\"{device}\" não tem saída em {sr} Hz."))?;
        let cfg = StreamConfig { channels: ch, sample_rate: sr, buffer_size: cpal::BufferSize::Default };
        let s = match fmt {
            SampleFormat::F32 => output::<f32>(&d, cfg, reader, levels, tap),
            SampleFormat::I32 => output::<i32>(&d, cfg, reader, levels, tap),
            SampleFormat::I24 => output::<cpal::I24>(&d, cfg, reader, levels, tap),
            _ => output::<i16>(&d, cfg, reader, levels, tap),
        }?;
        s.play().map_err(|e| e.to_string())?;
        Ok(Handle::Stream(s))
    }

    fn output<T>(d: &cpal::Device, cfg: StreamConfig, mut reader: Reader, levels: Arc<Levels>, tap: TapRef) -> Result<cpal::Stream, String>
    where
        T: SizedSample + FromSample<f32>,
    {
        let ch = cfg.channels as usize;
        let (mut l, mut r) = (vec![0.0f32; 16384], vec![0.0f32; 16384]);
        d.build_output_stream::<T, _, _>(
            cfg,
            move |data: &mut [T], _| {
                let frames = (data.len() / ch).min(16384);
                {
                    let mut outs: [&mut [f32]; 2] = [&mut l[..frames], &mut r[..frames]];
                    reader.pull_planar(&mut outs, frames);
                }
                if let Some(t) = &tap {
                    t.push_planar(&l[..frames], &r[..frames]);
                }
                let mut peak = 0.0f32;
                for i in 0..frames {
                    peak = peak.max(l[i].abs()).max(r[i].abs());
                    for c in 0..ch {
                        let v = match c {
                            0 => l[i],
                            1 => r[i],
                            _ => 0.0,
                        };
                        data[i * ch + c] = T::from_sample(v.clamp(-1.0, 1.0));
                    }
                }
                Levels::put(&levels.recv_peak, peak);
            },
            |e| tracing::warn!("saída: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
    }
}

/// Serve o áudio recebido ao plugin "Sona" do OBS (TCP só neste computador),
/// no ritmo do relógio deste computador: a deriva da rede já foi compensada
/// pela ponte, e o OBS usa o mesmo relógio para os instantes.
fn start_obs_feed(sample_rate: u32, mut reader: Reader, levels: Arc<Levels>, tap: TapRef) -> Result<Handle, String> {
    use sona_obs::feed;
    use std::io::Write;
    let listener = std::net::TcpListener::bind(feed::ADDR)
        .map_err(|e| format!("Porta do plugin do OBS ({}) ocupada: {e}. Outro Sona Bridge aberto?", feed::ADDR))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = stop.clone();
    let t = std::thread::Builder::new()
        .name("bridge-obs".into())
        .spawn(move || {
            const FRAMES: usize = 480; // 10 ms
            let header = feed::header_bytes(&feed::Header { sample_rate, channels: 2, frames: FRAMES as u16 });
            let period = Duration::from_secs_f64(FRAMES as f64 / sample_rate as f64);
            // Limite de atraso por cliente: 0,5 s; passou disso, desconecta (o plugin reconecta).
            let limit = sample_rate as usize / 2 * 2 * 4;
            let (mut l, mut r) = (vec![0.0f32; FRAMES], vec![0.0f32; FRAMES]);
            let mut bytes = Vec::with_capacity(FRAMES * 8);
            let mut clients: Vec<(std::net::TcpStream, Vec<u8>)> = Vec::new();
            let mut deadline = Instant::now();
            while !s2.load(Relaxed) {
                while let Ok((c, _)) = listener.accept() {
                    let _ = c.set_nonblocking(true);
                    let _ = c.set_nodelay(true);
                    clients.push((c, header.to_vec()));
                }
                {
                    let mut outs: [&mut [f32]; 2] = [&mut l, &mut r];
                    reader.pull_planar(&mut outs, FRAMES);
                }
                let peak = l.iter().chain(r.iter()).fold(0.0f32, |m, v| m.max(v.abs()));
                Levels::put(&levels.recv_peak, peak);
                if let Some(t) = &tap {
                    t.push_planar(&l, &r);
                }
                feed::interleave(&[&l, &r], FRAMES, &mut bytes);
                clients.retain_mut(|(c, pending)| {
                    pending.extend_from_slice(&bytes);
                    loop {
                        match c.write(pending) {
                            Ok(0) => return false,
                            Ok(n) => {
                                pending.drain(..n);
                                if pending.is_empty() {
                                    break;
                                }
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                            Err(_) => return false,
                        }
                    }
                    pending.len() < limit
                });
                deadline += period;
                let now = Instant::now();
                if deadline > now {
                    std::thread::sleep(deadline - now);
                } else if now - deadline > Duration::from_millis(200) {
                    deadline = now;
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(Handle::Thread(stop, Some(t)))
}
