//! Protocolo local entre o Sona Bridge e o plugin do OBS (só 127.0.0.1).
//!
//! Ao conectar, o bridge manda um cabeçalho de 16 bytes:
//! `"SONA-OBS"` | taxa u32 LE | canais u16 LE | quadros por bloco u16 LE.
//! Depois, blocos contínuos de `quadros × canais` amostras f32 LE intercaladas.

use std::io::Read;

pub const ADDR: &str = "127.0.0.1:7360";
pub const MAGIC: &[u8; 8] = b"SONA-OBS";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u16,
}

pub fn header_bytes(h: &Header) -> [u8; 16] {
    let mut b = [0u8; 16];
    b[..8].copy_from_slice(MAGIC);
    b[8..12].copy_from_slice(&h.sample_rate.to_le_bytes());
    b[12..14].copy_from_slice(&h.channels.to_le_bytes());
    b[14..16].copy_from_slice(&h.frames.to_le_bytes());
    b
}

pub fn read_header(r: &mut impl Read) -> std::io::Result<Header> {
    let mut b = [0u8; 16];
    r.read_exact(&mut b)?;
    if &b[..8] != MAGIC {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "não é o Sona Bridge"));
    }
    let h = Header {
        sample_rate: u32::from_le_bytes(b[8..12].try_into().unwrap()),
        channels: u16::from_le_bytes(b[12..14].try_into().unwrap()),
        frames: u16::from_le_bytes(b[14..16].try_into().unwrap()),
    };
    if !(8_000..=192_000).contains(&h.sample_rate) || !(1..=8).contains(&h.channels) || h.frames == 0 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "cabeçalho inválido"));
    }
    Ok(h)
}

/// f32 LE intercalado → planar.
pub fn deinterleave(raw: &[u8], planes: &mut [Vec<f32>]) {
    let ch = planes.len();
    for (k, c) in raw.chunks_exact(4).enumerate() {
        let v = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        if let Some(p) = planes.get_mut(k % ch).and_then(|p| p.get_mut(k / ch)) {
            *p = v;
        }
    }
}

/// Planar → f32 LE intercalado (lado do bridge).
pub fn interleave(planes: &[&[f32]], frames: usize, out: &mut Vec<u8>) {
    out.clear();
    for i in 0..frames {
        for p in planes {
            out.extend_from_slice(&p.get(i).copied().unwrap_or(0.0).to_le_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cabecalho_e_amostras_ida_e_volta() {
        let h = Header { sample_rate: 48_000, channels: 2, frames: 480 };
        let mut wire = header_bytes(&h).to_vec();
        let l: Vec<f32> = (0..480).map(|i| i as f32 / 480.0).collect();
        let r: Vec<f32> = l.iter().map(|v| -v).collect();
        let mut body = Vec::new();
        interleave(&[&l, &r], 480, &mut body);
        wire.extend_from_slice(&body);
        let mut cur = std::io::Cursor::new(wire);
        assert_eq!(read_header(&mut cur).unwrap(), h);
        let mut raw = vec![0u8; 480 * 2 * 4];
        cur.read_exact(&mut raw).unwrap();
        let mut planes = vec![vec![0.0; 480]; 2];
        deinterleave(&raw, &mut planes);
        assert_eq!(planes[0], l);
        assert_eq!(planes[1], r);
    }

    #[test]
    fn recusa_quem_nao_e_o_bridge() {
        let mut cur = std::io::Cursor::new(b"HTTP/1.1 200 OK\r\n".to_vec());
        assert!(read_header(&mut cur).is_err());
    }
}
