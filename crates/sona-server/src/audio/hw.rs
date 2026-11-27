//! Dispositivos reais via cpal (ASIO/WASAPI no Windows, ALSA/JACK/PipeWire no
//! Linux, CoreAudio no macOS).
//!
//! ESTADO: exercitado só com o ALSA "null" do container. Nada validado com a
//! Ui24R ou com cabos virtuais no Windows. Ver docs/spec/checklist.md.
//!
//! - Principal: o callback de saída dita o relógio do motor. A entrada do mesmo
//!   aparelho chega por uma fila SPSC (mesmo relógio, sem reamostragem).
//! - Secundário: stream próprio, ligado ao motor por uma ponte de deriva.
//! - Quando um stream é derrubado, o que ele carregava (motor ou ponte) volta
//!   por um canal para o gerenciador reabrir o aparelho.

use super::composite::Composite;
use super::EngineStats;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Serialize)]
pub struct ConfigRange {
    pub channels: u16,
    pub min_rate: u32,
    pub max_rate: u32,
    pub format: String,
}

#[derive(Debug, Serialize)]
pub struct DeviceInfo {
    pub host: String,
    pub name: String,
    pub inputs: Vec<ConfigRange>,
    pub outputs: Vec<ConfigRange>,
    pub default_input: Option<String>,
    pub default_output: Option<String>,
}

fn ranges<I: Iterator<Item = cpal::SupportedStreamConfigRange>>(it: Result<I, cpal::Error>) -> Vec<ConfigRange> {
    it.map(|i| {
        i.map(|r| ConfigRange {
            channels: r.channels(),
            min_rate: r.min_sample_rate(),
            max_rate: r.max_sample_rate(),
            format: format!("{:?}", r.sample_format()),
        })
        .collect()
    })
    .unwrap_or_default()
}

/// FR-01: lista os dispositivos de todos os hosts disponíveis.
pub fn list_devices() -> Vec<DeviceInfo> {
    let mut out = Vec::new();
    for id in cpal::available_hosts() {
        let Ok(host) = cpal::host_from_id(id) else { continue };
        let Ok(devices) = host.devices() else { continue };
        for d in devices {
            let name = d.description().map(|x| x.name().to_string()).unwrap_or_else(|_| d.to_string());
            out.push(DeviceInfo {
                host: id.name().to_string(),
                inputs: ranges(d.supported_input_configs()),
                outputs: ranges(d.supported_output_configs()),
                default_input: d
                    .default_input_config()
                    .ok()
                    .map(|c| format!("{} ch {} Hz {:?}", c.channels(), c.sample_rate(), c.sample_format())),
                default_output: d
                    .default_output_config()
                    .ok()
                    .map(|c| format!("{} ch {} Hz {:?}", c.channels(), c.sample_rate(), c.sample_format())),
                name,
            });
        }
    }
    out
}

/// Separa "ASIO:Nome" em (Some("ASIO"), "Nome"). Sem prefixo de host conhecido,
/// procura em todos os hosts.
pub fn split_host(spec: &str) -> (Option<&str>, &str) {
    for id in cpal::available_hosts() {
        let h = id.name();
        if let Some(rest) = spec.strip_prefix(h).and_then(|r| r.strip_prefix(':')) {
            return (Some(h), rest);
        }
    }
    (None, spec)
}

/// Acha o aparelho. Aceita "HOST:Nome" (ex.: "ASIO:Soundcraft…") ou só o nome.
/// Nome exato vence nome contido (os pares WASAPI da Soundcraft contêm o nome
/// do dispositivo ASIO); sem host, ASIO vence os outros.
pub fn find_device(spec: &str) -> Option<cpal::Device> {
    find_device_host(spec).map(|(d, _)| d)
}

/// Igual a `find_device`, devolvendo também o nome do host ("ASIO", "WASAPI"…).
pub fn find_device_host(spec: &str) -> Option<(cpal::Device, &'static str)> {
    let (want_host, name) = split_host(spec);
    let mut hosts: Vec<cpal::HostId> = cpal::available_hosts().into_iter().filter(|id| want_host.is_none_or(|h| id.name() == h)).collect();
    hosts.sort_by_key(|id| if id.name() == "ASIO" { 0 } else { 1 });
    let mut contains = None;
    for id in hosts {
        let Ok(host) = cpal::host_from_id(id) else { continue };
        let Ok(devices) = host.devices() else { continue };
        for d in devices {
            let n = d.description().map(|x| x.name().to_string()).unwrap_or_else(|_| d.to_string());
            if n == name {
                return Some((d, id.name()));
            }
            if contains.is_none() && n.contains(name) {
                contains = Some((d, id.name()));
            }
        }
    }
    contains
}

/// Canais máximos de entrada e saída que o aparelho oferece na taxa dada.
pub fn channels(name: &str, sample_rate: u32) -> Result<(u16, u16), String> {
    let d = find_device(name).ok_or_else(|| format!("\"{name}\" não encontrado."))?;
    let pick = |r: Vec<ConfigRange>| {
        r.into_iter().filter(|c| c.min_rate <= sample_rate && c.max_rate >= sample_rate).map(|c| c.channels).max().unwrap_or(0)
    };
    Ok((pick(ranges(d.supported_input_configs())), pick(ranges(d.supported_output_configs()))))
}

fn format_for(d: &cpal::Device, input: bool, channels: u16, sr: u32) -> Option<SampleFormat> {
    let list: Vec<_> = if input { d.supported_input_configs().ok()?.collect() } else { d.supported_output_configs().ok()?.collect() };
    let ok = |f: SampleFormat| {
        list.iter().any(|r| r.channels() == channels && r.sample_format() == f && r.min_sample_rate() <= sr && r.max_sample_rate() >= sr)
    };
    [SampleFormat::F32, SampleFormat::I32, SampleFormat::I24, SampleFormat::I16].into_iter().find(|f| ok(*f))
}

/// Guarda um valor dentro do callback e devolve pelo canal quando o stream é
/// derrubado (o callback é destruído junto com o stream).
struct Returning<T> {
    value: Option<T>,
    back: Sender<T>,
}

impl<T> Drop for Returning<T> {
    fn drop(&mut self) {
        if let Some(v) = self.value.take() {
            let _ = self.back.send(v);
        }
    }
}

const MAX_FRAMES: usize = 4096;

pub struct PrimaryHandle {
    _input: Option<cpal::Stream>,
    output: Option<cpal::Stream>,
    back: Receiver<Composite>,
    pub failed: Arc<AtomicBool>,
}

impl PrimaryHandle {
    /// Derruba os streams e devolve o motor.
    pub fn stop(mut self) -> Option<Composite> {
        self._input.take();
        self.output.take();
        self.back.recv_timeout(std::time::Duration::from_secs(2)).ok()
    }
}

/// Abre o principal. Em erro, devolve o motor junto com a mensagem.
pub fn start_primary(
    name: &str,
    comp: Composite,
    ch_in: u16,
    ch_out: u16,
    sample_rate: u32,
    block: usize,
    stats: Arc<EngineStats>,
) -> Result<PrimaryHandle, (String, Composite)> {
    let Some((d, host)) = find_device_host(name) else { return Err((format!("\"{name}\" não encontrado."), comp)) };
    if ch_out == 0 {
        return Err((format!("\"{name}\" não tem saída; o principal precisa de saída para ditar o relógio."), comp));
    }
    let Some(fmt_out) = format_for(&d, false, ch_out, sample_rate) else {
        return Err((format!("\"{name}\": formato de saída não suportado em {sample_rate} Hz."), comp));
    };
    let fmt_in = if ch_in > 0 { format_for(&d, true, ch_in, sample_rate) } else { None };
    // ASIO: o tamanho do bloco é do driver (painel da Soundcraft); pedir outro
    // costuma falhar. Nos outros hosts, pede o bloco configurado.
    let buffer_size = if host == "ASIO" { cpal::BufferSize::Default } else { cpal::BufferSize::Fixed(block as u32) };
    let cfg = |ch| StreamConfig { channels: ch, sample_rate, buffer_size };
    let failed = Arc::new(AtomicBool::new(false));
    let f1 = failed.clone();
    let err = move |e: cpal::Error| {
        tracing::error!("principal: {e}");
        f1.store(true, Relaxed);
    };

    // Entrada do mesmo aparelho: fila de 8 blocos, pré-carga de 1.
    let nin = if fmt_in.is_some() { ch_in as usize } else { 0 };
    let (mut prod, mut cons) = rtrb::RingBuffer::<f32>::new(block * nin.max(1) * 8);
    for _ in 0..block * nin {
        let _ = prod.push(0.0);
    }
    let input = match fmt_in {
        Some(SampleFormat::F32) => Some(build_input::<f32>(&d, cfg(ch_in), prod, err.clone())),
        Some(SampleFormat::I32) => Some(build_input::<i32>(&d, cfg(ch_in), prod, err.clone())),
        Some(SampleFormat::I24) => Some(build_input::<cpal::I24>(&d, cfg(ch_in), prod, err.clone())),
        Some(_) => Some(build_input::<i16>(&d, cfg(ch_in), prod, err.clone())),
        None => None,
    };
    let input = match input.transpose() {
        Ok(i) => i,
        Err(e) => return Err((e, comp)),
    };

    let nout = ch_out as usize;
    let (back_tx, back_rx) = channel();
    let mut holder = Returning { value: Some(comp), back: back_tx };
    let mut ins = vec![vec![0.0f32; MAX_FRAMES]; nin];
    let mut outs = vec![vec![0.0f32; MAX_FRAMES]; nout];
    let stats2 = stats.clone();
    let max_block = holder.value.as_ref().unwrap().max_block();
    let mut process = move |frames: usize, write: &mut dyn FnMut(usize, usize, f32)| {
        let t0 = Instant::now();
        let Some(comp) = holder.value.as_mut() else { return };
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(max_block);
            if cons.slots() >= n * nin {
                for i in 0..n {
                    for c in 0..nin {
                        ins[c][i] = cons.pop().unwrap_or(0.0);
                    }
                }
            } else {
                stats2.input_starved.fetch_add(1, Relaxed);
                ins.iter_mut().for_each(|b| b[..n].fill(0.0));
            }
            {
                let in_refs: [&[f32]; 64] = std::array::from_fn(|i| ins.get(i).map(|v| &v[..n]).unwrap_or(&[]));
                let mut it = outs.iter_mut();
                let mut out_refs: [&mut [f32]; 64] = std::array::from_fn(|_| it.next().map(|v| &mut v[..n]).unwrap_or(&mut []));
                comp.run(&in_refs[..nin.min(64)], &mut out_refs[..nout.min(64)], n);
            }
            for i in 0..n {
                for c in 0..nout {
                    write(done + i, c, outs[c][i]);
                }
            }
            done += n;
        }
        stats2.record(t0.elapsed().as_secs_f64(), frames as f64 / sample_rate as f64);
    };
    let built = match fmt_out {
        SampleFormat::F32 => d.build_output_stream::<f32, _, _>(cfg(ch_out), move |data, _| run_out(data, nout, &mut process), err, None),
        SampleFormat::I32 => d.build_output_stream::<i32, _, _>(cfg(ch_out), move |data, _| run_out(data, nout, &mut process), err, None),
        SampleFormat::I24 => {
            d.build_output_stream::<cpal::I24, _, _>(cfg(ch_out), move |data, _| run_out(data, nout, &mut process), err, None)
        }
        _ => d.build_output_stream::<i16, _, _>(cfg(ch_out), move |data, _| run_out(data, nout, &mut process), err, None),
    };
    let output = match built {
        Ok(o) => o,
        // O callback (e o motor dentro dele) já foi destruído: o motor volta pelo canal.
        Err(e) => return Err((format!("Saída de \"{name}\": {e}"), back_rx.recv().expect("motor devolvido"))),
    };
    if let Some(i) = &input {
        if let Err(e) = i.play() {
            drop(output);
            return Err((format!("Entrada de \"{name}\": {e}"), back_rx.recv().expect("motor devolvido")));
        }
    }
    if let Err(e) = output.play() {
        drop(output);
        return Err((format!("Saída de \"{name}\": {e}"), back_rx.recv().expect("motor devolvido")));
    }
    stats.sample_rate.store(sample_rate, Relaxed);
    stats.block.store(block as u32, Relaxed);
    stats.running.store(true, Relaxed);
    Ok(PrimaryHandle { _input: input, output: Some(output), back: back_rx, failed })
}

fn run_out<T: SizedSample + FromSample<f32>>(
    data: &mut [T],
    nout: usize,
    process: &mut impl FnMut(usize, &mut dyn FnMut(usize, usize, f32)),
) {
    let frames = data.len() / nout.max(1);
    process(frames, &mut |i: usize, c: usize, v: f32| data[i * nout + c] = T::from_sample(v.clamp(-1.0, 1.0)));
}

fn build_input<T>(
    d: &cpal::Device,
    cfg: StreamConfig,
    mut prod: rtrb::Producer<f32>,
    err: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    d.build_input_stream::<T, _, _>(
        cfg,
        move |data: &[T], _| {
            for s in data {
                // Fila cheia: o lado de saída parou. Descarta em vez de bloquear.
                let _ = prod.push(<f32 as FromSample<T>>::from_sample_(*s));
            }
        },
        err,
        None,
    )
    .map_err(|e| format!("Entrada: {e}"))
}

// ── Secundários ──────────────────────────────────────────────────

pub enum SecEnd {
    /// Aparelho de entrada: o callback escreve na ponte.
    In(sona_core::drift::Writer),
    /// Aparelho de saída: o callback lê da ponte (com reamostragem).
    Out(sona_core::drift::Reader),
}

pub struct SecondaryHandle {
    stream: Option<cpal::Stream>,
    back: Receiver<SecEnd>,
    pub failed: Arc<AtomicBool>,
}

impl SecondaryHandle {
    pub fn stop(mut self) -> Option<SecEnd> {
        self.stream.take();
        self.back.recv_timeout(std::time::Duration::from_secs(2)).ok()
    }
}

/// Abre um secundário. Em erro, devolve a ponte junto com a mensagem.
pub fn start_secondary(
    name: &str,
    end: SecEnd,
    channels: u16,
    sample_rate: u32,
    block: usize,
) -> Result<SecondaryHandle, (String, SecEnd)> {
    let Some(d) = find_device(name) else { return Err((format!("\"{name}\" não encontrado."), end)) };
    let input = matches!(end, SecEnd::In(_));
    // Usa no máximo o que o aparelho oferece; canais a mais ficam em silêncio.
    let avail = match channels_of(&d, input, sample_rate) {
        0 => return Err((format!("\"{name}\" não tem {} em {sample_rate} Hz.", if input { "entrada" } else { "saída" }), end)),
        n => n,
    };
    let dev_ch = if avail >= channels { channels } else { avail };
    let Some(fmt) = format_for(&d, input, dev_ch, sample_rate) else { return Err((format!("\"{name}\": formato não suportado."), end)) };
    let cfg = StreamConfig { channels: dev_ch, sample_rate, buffer_size: cpal::BufferSize::Default };
    let _ = block;
    let failed = Arc::new(AtomicBool::new(false));
    let f1 = failed.clone();
    let label = name.to_string();
    let err = move |e: cpal::Error| {
        tracing::warn!("{label}: {e}");
        f1.store(true, Relaxed);
    };
    let (tx, rx) = channel();
    let built = match end {
        SecEnd::In(w) => {
            let holder = Returning { value: Some(SecEnd::In(w)), back: tx };
            match fmt {
                SampleFormat::F32 => sec_input::<f32>(&d, cfg, holder, err),
                SampleFormat::I32 => sec_input::<i32>(&d, cfg, holder, err),
                SampleFormat::I24 => sec_input::<cpal::I24>(&d, cfg, holder, err),
                _ => sec_input::<i16>(&d, cfg, holder, err),
            }
        }
        SecEnd::Out(r) => {
            let holder = Returning { value: Some(SecEnd::Out(r)), back: tx };
            match fmt {
                SampleFormat::F32 => sec_output::<f32>(&d, cfg, holder, err),
                SampleFormat::I32 => sec_output::<i32>(&d, cfg, holder, err),
                SampleFormat::I24 => sec_output::<cpal::I24>(&d, cfg, holder, err),
                _ => sec_output::<i16>(&d, cfg, holder, err),
            }
        }
    };
    match built.and_then(|s| s.play().map(|_| s).map_err(|e| e.to_string())) {
        Ok(s) => Ok(SecondaryHandle { stream: Some(s), back: rx, failed }),
        Err(e) => Err((format!("\"{name}\": {e}"), rx.recv().expect("ponte devolvida"))),
    }
}

fn channels_of(d: &cpal::Device, input: bool, sr: u32) -> u16 {
    let r = if input { ranges(d.supported_input_configs()) } else { ranges(d.supported_output_configs()) };
    r.into_iter().filter(|c| c.min_rate <= sr && c.max_rate >= sr).map(|c| c.channels).max().unwrap_or(0)
}

fn sec_input<T>(
    d: &cpal::Device,
    cfg: StreamConfig,
    mut holder: Returning<SecEnd>,
    err: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let dev_ch = cfg.channels as usize;
    let mut scratch = vec![0.0f32; MAX_FRAMES * 32];
    d.build_input_stream::<T, _, _>(
        cfg,
        move |data: &[T], _| {
            let Some(SecEnd::In(w)) = holder.value.as_mut() else { return };
            let ch = w.channels();
            let frames = (data.len() / dev_ch).min(MAX_FRAMES);
            for i in 0..frames {
                for c in 0..ch {
                    scratch[i * ch + c] = if c < dev_ch { <f32 as FromSample<T>>::from_sample_(data[i * dev_ch + c]) } else { 0.0 };
                }
            }
            w.push_interleaved(&scratch[..frames * ch]);
        },
        err,
        None,
    )
    .map_err(|e| e.to_string())
}

fn sec_output<T>(
    d: &cpal::Device,
    cfg: StreamConfig,
    mut holder: Returning<SecEnd>,
    err: impl FnMut(cpal::Error) + Send + 'static,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let dev_ch = cfg.channels as usize;
    let mut planes: Vec<Vec<f32>> = vec![vec![0.0f32; MAX_FRAMES]; 32];
    d.build_output_stream::<T, _, _>(
        cfg,
        move |data: &mut [T], _| {
            let Some(SecEnd::Out(r)) = holder.value.as_mut() else { return };
            let ch = r.channels().min(32);
            let frames = (data.len() / dev_ch).min(MAX_FRAMES);
            {
                let mut refs: [&mut [f32]; 32] = Default::default();
                for (rf, p) in refs.iter_mut().zip(planes.iter_mut()) {
                    *rf = &mut p[..frames];
                }
                r.pull_planar(&mut refs[..ch], frames);
            }
            for i in 0..frames {
                for c in 0..dev_ch {
                    let v = if c < ch { planes[c][i] } else { 0.0 };
                    data[i * dev_ch + c] = T::from_sample(v.clamp(-1.0, 1.0));
                }
            }
        },
        err,
        None,
    )
    .map_err(|e| e.to_string())
}
