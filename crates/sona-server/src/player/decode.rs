//! Leitura de uma faixa (WAV, AIFF, FLAC, MP3) em quadros estéreo na taxa do
//! motor. Roda na thread de disco, nunca na de áudio.
//!
//! Mono vira L = R. Taxa diferente da do motor (ex.: 44,1 kHz) é convertida com
//! interpolação cúbica (Catmull-Rom), a mesma da ponte de deriva.

use std::path::Path;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Timestamp;

#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    pub channels: u16,
    pub sample_rate: u32,
    /// Duração em quadros da própria faixa (na taxa dela).
    pub frames: u64,
}

pub struct TrackReader {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    pub info: Info,
    out_rate: u32,
    /// Quadros de origem já decodificados (estéreo), a partir de `head`.
    fifo: Vec<[f32; 2]>,
    head: usize,
    /// Posição fracionária entre fifo[head] e fifo[head + 1].
    frac: f64,
    step: f64,
    /// Quadros de origem a descartar (busca caiu antes do ponto pedido).
    skip: u64,
    eof: bool,
    tmp: Vec<f32>,
}

fn open_format(path: &Path) -> Result<Box<dyn FormatReader>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| format!("Formato não reconhecido ({e}). Use WAV, AIFF, FLAC ou MP3."))
}

/// Canais, taxa e duração de um arquivo.
pub fn probe(path: &Path) -> Result<Info, String> {
    let format = open_format(path)?;
    let track = format.default_track(TrackType::Audio).ok_or("O arquivo não tem áudio.")?;
    let params = track.codec_params.as_ref().and_then(|p| p.audio()).ok_or("O arquivo não tem áudio.")?;
    let sample_rate = params.sample_rate.ok_or("Taxa de amostragem desconhecida.")?;
    let channels = params.channels.as_ref().map(|c| c.count()).unwrap_or(0);
    if channels == 0 {
        return Err("Número de canais desconhecido.".into());
    }
    let mut frames = track.num_frames.unwrap_or(0);
    if frames == 0 {
        // MP3 sem cabeçalho de duração: conta decodificando.
        let mut r = TrackReader::open(path, sample_rate)?;
        let mut buf = vec![0.0f32; 8192];
        loop {
            let n = r.read(&mut buf);
            frames += n as u64;
            if n < buf.len() / 2 {
                break;
            }
        }
    }
    Ok(Info { channels: channels.min(u16::MAX as usize) as u16, sample_rate, frames })
}

impl TrackReader {
    pub fn open(path: &Path, out_rate: u32) -> Result<TrackReader, String> {
        let format = open_format(path)?;
        let track = format.default_track(TrackType::Audio).ok_or("O arquivo não tem áudio.")?;
        let params = track.codec_params.as_ref().and_then(|p| p.audio()).ok_or("O arquivo não tem áudio.")?.clone();
        let track_id = track.id;
        let sample_rate = params.sample_rate.ok_or("Taxa de amostragem desconhecida.")?;
        let channels = params.channels.as_ref().map(|c| c.count()).unwrap_or(1).max(1);
        let frames = track.num_frames.unwrap_or(0);
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .map_err(|e| format!("Codificação não suportada ({e})."))?;
        Ok(TrackReader {
            format,
            decoder,
            track_id,
            info: Info { channels: channels as u16, sample_rate, frames },
            out_rate,
            fifo: Vec::with_capacity(16384),
            head: 0,
            frac: 0.0,
            step: sample_rate as f64 / out_rate as f64,
            skip: 0,
            eof: false,
            tmp: Vec::new(),
        })
    }

    /// Vai para o quadro `out_frame` (na taxa do motor).
    pub fn seek(&mut self, out_frame: u64) {
        let src = (out_frame as f64 * self.step).round() as u64;
        self.fifo.clear();
        self.head = 0;
        self.frac = 0.0;
        self.skip = 0;
        self.eof = false;
        if src == 0 {
            // Reabrir é mais simples e exato que buscar o início.
            let _ = self.format.seek(SeekMode::Accurate, SeekTo::Timestamp { ts: Timestamp::new(0), track_id: self.track_id });
            self.decoder.reset();
            return;
        }
        match self.format.seek(SeekMode::Accurate, SeekTo::Timestamp { ts: Timestamp::new(src as i64), track_id: self.track_id }) {
            Ok(to) => {
                self.decoder.reset();
                self.skip = (to.required_ts.get() - to.actual_ts.get()).max(0) as u64;
            }
            Err(_) => self.eof = true,
        }
    }

    /// Decodifica mais um pacote para a fila. `false` no fim do arquivo.
    fn refill(&mut self) -> bool {
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return false,
                Err(Error::ResetRequired) => {
                    self.decoder.reset();
                    continue;
                }
                Err(_) => return false,
            };
            if packet.track_id != self.track_id {
                continue;
            }
            let buf = match self.decoder.decode(&packet) {
                Ok(b) => b,
                Err(Error::DecodeError(_)) => continue,
                Err(_) => return false,
            };
            let ch = buf.spec().channels().count().max(1);
            buf.copy_to_vec_interleaved::<f32>(&mut self.tmp);
            if self.head > 4096 {
                self.fifo.drain(..self.head - 1);
                self.head = 1;
            }
            for f in self.tmp.chunks_exact(ch) {
                if self.skip > 0 {
                    self.skip -= 1;
                    continue;
                }
                let l = f[0];
                let r = if ch > 1 { f[1] } else { l };
                self.fifo.push([l, r]);
            }
            return true;
        }
    }

    /// Escreve quadros estéreo intercalados em `out`. Devolve quantos quadros
    /// escreveu (menos que o pedido = fim da faixa; o resto fica com zero).
    pub fn read(&mut self, out: &mut [f32]) -> usize {
        let want = out.len() / 2;
        let mut done = 0;
        if self.step == 1.0 {
            while done < want {
                if self.head >= self.fifo.len() {
                    self.fifo.clear();
                    self.head = 0;
                    if self.eof || !self.refill() {
                        self.eof = true;
                        break;
                    }
                    continue;
                }
                let [l, r] = self.fifo[self.head];
                out[2 * done] = l;
                out[2 * done + 1] = r;
                self.head += 1;
                done += 1;
            }
        } else {
            while done < want {
                // Precisa de fifo[head - 1 ..= head + 2].
                while self.head + 2 >= self.fifo.len() && !self.eof {
                    if !self.refill() {
                        self.eof = true;
                    }
                }
                if self.head >= self.fifo.len() {
                    break;
                }
                let get = |i: isize| -> [f32; 2] {
                    if i < 0 || i as usize >= self.fifo.len() {
                        [0.0, 0.0]
                    } else {
                        self.fifo[i as usize]
                    }
                };
                let h = self.head as isize;
                let (x0, x1, x2, x3) = (get(h - 1), get(h), get(h + 1), get(h + 2));
                let t = self.frac as f32;
                for c in 0..2 {
                    let (a, b, cc, d) = (x0[c], x1[c], x2[c], x3[c]);
                    out[2 * done + c] = b + 0.5 * t * (cc - a + t * (2.0 * a - 5.0 * b + 4.0 * cc - d + t * (3.0 * (b - cc) + d - a)));
                }
                done += 1;
                self.frac += self.step;
                while self.frac >= 1.0 {
                    self.frac -= 1.0;
                    self.head += 1;
                }
            }
        }
        out[2 * done..].fill(0.0);
        done
    }

    /// Duração na taxa do motor.
    pub fn out_frames(&self) -> u64 {
        (self.info.frames as f64 * self.out_rate as f64 / self.info.sample_rate as f64).round() as u64
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// WAV PCM 16 bits com um seno (para testes).
    pub fn write_wav(path: &Path, rate: u32, channels: u16, seconds: f32, freq: f32) {
        let frames = (rate as f32 * seconds) as u32;
        let mut data = Vec::with_capacity(frames as usize * channels as usize * 2);
        for i in 0..frames {
            let v = (0.5 * (std::f32::consts::TAU * freq * i as f32 / rate as f32).sin() * 32767.0) as i16;
            for _ in 0..channels {
                data.extend_from_slice(&v.to_le_bytes());
            }
        }
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&channels.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
        b.extend_from_slice(&(channels * 2).to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&(data.len() as u32).to_le_bytes());
        b.extend_from_slice(&data);
        std::fs::write(path, b).unwrap();
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("sona-decode-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d.join(name)
    }

    #[test]
    fn le_wav_mono_na_mesma_taxa() {
        let p = tmp("mono.wav");
        write_wav(&p, 48_000, 1, 0.5, 440.0);
        assert_eq!(probe(&p).unwrap(), Info { channels: 1, sample_rate: 48_000, frames: 24_000 });
        let mut r = TrackReader::open(&p, 48_000).unwrap();
        assert_eq!(r.out_frames(), 24_000);
        let mut buf = vec![0.0f32; 2 * 30_000];
        let n = r.read(&mut buf);
        assert_eq!(n, 24_000);
        assert_eq!(buf[200], buf[201], "mono vira L = R");
        let peak = buf.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((peak - 0.5).abs() < 0.01, "pico {peak}");
    }

    #[test]
    fn converte_44k1_para_48k_e_busca() {
        let p = tmp("est.wav");
        write_wav(&p, 44_100, 2, 1.0, 1000.0);
        let mut r = TrackReader::open(&p, 48_000).unwrap();
        assert_eq!(r.out_frames(), 48_000);
        let mut buf = vec![0.0f32; 2 * 50_000];
        let n = r.read(&mut buf);
        assert!((47_990..=48_001).contains(&n), "{n}");
        // Frequência preservada: conta cruzamentos por zero em 0,5 s (~1000 ciclos/s).
        let cross = buf[..48_000].chunks(2).collect::<Vec<_>>().windows(2).filter(|w| w[0][0] < 0.0 && w[1][0] >= 0.0).count();
        assert!((495..=505).contains(&cross), "{cross}");
        r.seek(24_000);
        let n2 = r.read(&mut buf);
        assert!((23_990..=24_001).contains(&n2), "{n2}");
    }
}
