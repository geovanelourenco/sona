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
pub fn start_send(device: &str, sample_rate: u32, ppm: f64, mut out: rtrb::Producer<f32>, levels: Arc<Levels>) -> Result<Handle, String> {
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
                    for s in &buf {
                        let _ = out.push(*s);
                    }
                    deadline += period;
                    let now = Instant::now();
                    if deadline > now {
                        std::thread::sleep(deadline - now);
                    } else if now - deadline > period * 4 {
                        deadline = now;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        return Ok(Handle::Thread(stop, Some(t)));
    }
    #[cfg(feature = "hw")]
    {
        hw::send(device, sample_rate, out, levels)
    }
    #[cfg(not(feature = "hw"))]
    Err(format!("\"{device}\": compilado sem suporte a aparelhos reais."))
}

/// Liga o aparelho de recepção: lê estéreo de `reader` (com deriva compensada).
pub fn start_receive(device: &str, sample_rate: u32, ppm: f64, mut reader: Reader, levels: Arc<Levels>) -> Result<Handle, String> {
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
                    } else if now - deadline > period * 4 {
                        deadline = now;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        return Ok(Handle::Thread(stop, Some(t)));
    }
    #[cfg(feature = "hw")]
    {
        hw::receive(device, sample_rate, reader, levels)
    }
    #[cfg(not(feature = "hw"))]
    Err(format!("\"{device}\": compilado sem suporte a aparelhos reais."))
}

#[cfg(feature = "hw")]
pub mod hw {
    use super::{Handle, Levels};
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

    fn find(name: &str) -> Option<cpal::Device> {
        for id in cpal::available_hosts() {
            let Ok(host) = cpal::host_from_id(id) else { continue };
            let Ok(devs) = host.devices() else { continue };
            for d in devs {
                let n = name_of(&d);
                if n == name || n.contains(name) {
                    return Some(d);
                }
            }
        }
        None
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

    pub fn send(device: &str, sr: u32, out: rtrb::Producer<f32>, levels: Arc<Levels>) -> Result<Handle, String> {
        let d = find(device).ok_or_else(|| format!("\"{device}\" não encontrado."))?;
        let (ch, fmt) = pick(&d, true, sr).ok_or_else(|| format!("\"{device}\" não tem entrada em {sr} Hz."))?;
        let cfg = StreamConfig { channels: ch, sample_rate: sr, buffer_size: cpal::BufferSize::Default };
        let s = match fmt {
            SampleFormat::F32 => input::<f32>(&d, cfg, out, levels),
            SampleFormat::I32 => input::<i32>(&d, cfg, out, levels),
            SampleFormat::I24 => input::<cpal::I24>(&d, cfg, out, levels),
            _ => input::<i16>(&d, cfg, out, levels),
        }?;
        s.play().map_err(|e| e.to_string())?;
        Ok(Handle::Stream(s))
    }

    fn input<T>(d: &cpal::Device, cfg: StreamConfig, mut out: rtrb::Producer<f32>, levels: Arc<Levels>) -> Result<cpal::Stream, String>
    where
        T: SizedSample,
        f32: FromSample<T>,
    {
        let ch = cfg.channels as usize;
        d.build_input_stream::<T, _, _>(
            cfg,
            move |data: &[T], _| {
                let mut peak = 0.0f32;
                for f in data.chunks(ch) {
                    let l = <f32 as FromSample<T>>::from_sample_(f[0]);
                    let r = if ch > 1 { <f32 as FromSample<T>>::from_sample_(f[1]) } else { l };
                    peak = peak.max(l.abs()).max(r.abs());
                    let _ = out.push(l);
                    let _ = out.push(r);
                }
                Levels::put(&levels.send_peak, peak);
            },
            |e| tracing::warn!("entrada: {e}"),
            None,
        )
        .map_err(|e| e.to_string())
    }

    pub fn receive(device: &str, sr: u32, reader: Reader, levels: Arc<Levels>) -> Result<Handle, String> {
        let d = find(device).ok_or_else(|| format!("\"{device}\" não encontrado."))?;
        let (ch, fmt) = pick(&d, false, sr).ok_or_else(|| format!("\"{device}\" não tem saída em {sr} Hz."))?;
        let cfg = StreamConfig { channels: ch, sample_rate: sr, buffer_size: cpal::BufferSize::Default };
        let s = match fmt {
            SampleFormat::F32 => output::<f32>(&d, cfg, reader, levels),
            SampleFormat::I32 => output::<i32>(&d, cfg, reader, levels),
            SampleFormat::I24 => output::<cpal::I24>(&d, cfg, reader, levels),
            _ => output::<i16>(&d, cfg, reader, levels),
        }?;
        s.play().map_err(|e| e.to_string())?;
        Ok(Handle::Stream(s))
    }

    fn output<T>(d: &cpal::Device, cfg: StreamConfig, mut reader: Reader, levels: Arc<Levels>) -> Result<cpal::Stream, String>
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
