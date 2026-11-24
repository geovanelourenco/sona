//! Dispositivos reais via cpal (ASIO/WASAPI no Windows, ALSA/JACK/PipeWire no
//! Linux, CoreAudio no macOS).
//!
//! ESTADO: compilado e coberto só pela listagem de dispositivos. O caminho
//! duplex NÃO foi validado com a Ui24R nem com outro hardware. Ver
//! docs/spec/checklist.md.
//!
//! Duplex: o callback de entrada põe amostras intercaladas numa fila SPSC; o
//! callback de saída tira um bloco, roda o motor e escreve a saída. Entrada e
//! saída do mesmo aparelho são tratadas como o mesmo relógio. Deriva entre
//! relógios diferentes ainda não é compensada (fase 3).

use super::{EngineStats, IoLayout, Port};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use serde::Serialize;
use sona_core::Engine;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::Instant;

pub const HW_DOMAIN: u32 = 10;

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

fn find_device(name: &str) -> Option<cpal::Device> {
    for id in cpal::available_hosts() {
        let Ok(host) = cpal::host_from_id(id) else { continue };
        let Ok(devices) = host.devices() else { continue };
        for d in devices {
            let n = d.description().map(|x| x.name().to_string()).unwrap_or_else(|_| d.to_string());
            if n == name || n.contains(name) {
                return Some(d);
            }
        }
    }
    None
}

pub struct HwHandle {
    _input: cpal::Stream,
    _output: cpal::Stream,
}

/// Lê quantos canais o aparelho oferece e devolve o layout plano.
pub fn probe_layout(name: &str, sample_rate: u32) -> Result<(IoLayout, u16, u16), String> {
    let d = find_device(name).ok_or_else(|| format!("Dispositivo \"{name}\" não encontrado."))?;
    let pick = |r: Vec<ConfigRange>| {
        r.into_iter().filter(|c| c.min_rate <= sample_rate && c.max_rate >= sample_rate).map(|c| c.channels).max().unwrap_or(0)
    };
    let ch_in = pick(ranges(d.supported_input_configs()));
    let ch_out = pick(ranges(d.supported_output_configs()));
    if ch_in == 0 && ch_out == 0 {
        return Err(format!("\"{name}\" não aceita {sample_rate} Hz."));
    }
    let port = |n: String| Port { name: n, device: name.into(), domain: HW_DOMAIN, connected: true };
    Ok((
        IoLayout {
            inputs: (1..=ch_in).map(|i| port(format!("In {i}"))).collect(),
            outputs: (1..=ch_out).map(|i| port(format!("Out {i}"))).collect(),
        },
        ch_in,
        ch_out,
    ))
}

fn format_for(d: &cpal::Device, input: bool, channels: u16, sr: u32) -> Option<SampleFormat> {
    let list: Vec<_> = if input { d.supported_input_configs().ok()?.collect() } else { d.supported_output_configs().ok()?.collect() };
    let ok = |f: SampleFormat| {
        list.iter().any(|r| r.channels() == channels && r.sample_format() == f && r.min_sample_rate() <= sr && r.max_sample_rate() >= sr)
    };
    [SampleFormat::F32, SampleFormat::I32, SampleFormat::I24, SampleFormat::I16].into_iter().find(|f| ok(*f))
}

pub fn start(name: &str, mut engine: Engine, sample_rate: u32, block: usize, stats: Arc<EngineStats>) -> Result<HwHandle, String> {
    let d = find_device(name).ok_or_else(|| format!("Dispositivo \"{name}\" não encontrado."))?;
    let (_, ch_in, ch_out) = probe_layout(name, sample_rate)?;
    let fmt_in = format_for(&d, true, ch_in, sample_rate).ok_or("Formato de entrada não suportado.")?;
    let fmt_out = format_for(&d, false, ch_out, sample_rate).ok_or("Formato de saída não suportado.")?;
    let cfg = |ch| StreamConfig { channels: ch, sample_rate, buffer_size: cpal::BufferSize::Fixed(block as u32) };

    // Fila de entrada: 8 blocos. Pré-carga de 1 bloco absorve a fase entre os callbacks.
    let (mut prod, mut cons) = rtrb::RingBuffer::<f32>::new(block * ch_in.max(1) as usize * 8);
    for _ in 0..block * ch_in as usize {
        let _ = prod.push(0.0);
    }
    stats.sample_rate.store(sample_rate, Relaxed);
    stats.block.store(block as u32, Relaxed);
    let err = |e: cpal::Error| tracing::error!("stream de áudio: {e}");

    let input = match fmt_in {
        SampleFormat::F32 => build_input::<f32>(&d, cfg(ch_in), prod, err),
        SampleFormat::I32 => build_input::<i32>(&d, cfg(ch_in), prod, err),
        SampleFormat::I24 => build_input::<cpal::I24>(&d, cfg(ch_in), prod, err),
        _ => build_input::<i16>(&d, cfg(ch_in), prod, err),
    }?;

    const MAX: usize = 4096;
    let (nin, nout) = (ch_in as usize, ch_out as usize);
    let mut ins = vec![vec![0.0f32; MAX]; nin];
    let mut outs = vec![vec![0.0f32; MAX]; nout];
    let stats2 = stats.clone();
    let mut process = move |frames: usize, write: &mut dyn FnMut(usize, usize, f32)| {
        let t0 = Instant::now();
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(MAX);
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
                engine.process(&in_refs[..nin.min(64)], &mut out_refs[..nout.min(64)], n);
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

    let output = match fmt_out {
        SampleFormat::F32 => d.build_output_stream::<f32, _, _>(cfg(ch_out), move |data, _| run_out(data, nout, &mut process), err, None),
        SampleFormat::I32 => d.build_output_stream::<i32, _, _>(cfg(ch_out), move |data, _| run_out(data, nout, &mut process), err, None),
        SampleFormat::I24 => {
            d.build_output_stream::<cpal::I24, _, _>(cfg(ch_out), move |data, _| run_out(data, nout, &mut process), err, None)
        }
        _ => d.build_output_stream::<i16, _, _>(cfg(ch_out), move |data, _| run_out(data, nout, &mut process), err, None),
    }
    .map_err(|e| format!("Saída: {e}"))?;

    input.play().map_err(|e| format!("Entrada: {e}"))?;
    output.play().map_err(|e| format!("Saída: {e}"))?;
    stats.running.store(true, Relaxed);
    Ok(HwHandle { _input: input, _output: output })
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
