//! Protocolo de áudio em rede do Sona (ver docs/adr/0003-transporte-de-rede.md).
//!
//! Pacote UDP, little-endian:
//!
//! ```text
//! 0   4  magic "SONA"
//! 4   1  versão (1)
//! 5   1  tipo (1 = áudio, 2 = sinal de vida)
//! 6   1  canais
//! 7   1  reservado
//! 8   4  sessão
//! 12  4  sequência
//! 16  8  instante (índice da amostra no relógio de quem envia)
//! 24  2  quadros
//! 26  2  reservado
//! 28  8  assinatura: HMAC-SHA256(chave da sessão, bytes 0..28 + carga) truncado
//! 36  …  carga: quadros × canais amostras PCM 24 bits com sinal, intercaladas
//! ```
//!
//! A chave de 16 bytes é entregue pelo canal de controle autenticado
//! (WebSocket com token de sessão). Pacote sem assinatura válida é descartado.

pub mod discovery;

use sha2::{Digest, Sha256};

pub const MAGIC: &[u8; 4] = b"SONA";
pub const VERSION: u8 = 1;
pub const HEADER: usize = 36;
pub const KIND_AUDIO: u8 = 1;
pub const KIND_ALIVE: u8 = 2;
/// Quadros por pacote: 120 a 48 kHz = 2,5 ms.
pub const FRAMES: usize = 120;
pub const MAX_CHANNELS: usize = 8;
pub const MAX_PACKET: usize = HEADER + FRAMES * MAX_CHANNELS * 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub kind: u8,
    pub channels: u8,
    pub session: u32,
    pub seq: u32,
    pub timestamp: u64,
    pub frames: u16,
}

pub type Key = [u8; 16];

/// HMAC-SHA256 (RFC 2104).
pub fn hmac(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    for p in parts {
        inner.update(p);
    }
    let ih = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(ih);
    outer.finalize().into()
}

fn eq_ct(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[inline]
fn to_i24(v: f32) -> [u8; 3] {
    let i = (v.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32;
    let b = i.to_le_bytes();
    [b[0], b[1], b[2]]
}

#[inline]
fn from_i24(b: &[u8]) -> f32 {
    let i = i32::from_le_bytes([b[0], b[1], b[2], if b[2] & 0x80 != 0 { 0xff } else { 0 }]);
    i as f32 / 8_388_607.0
}

/// Monta um pacote em `out` (sem alocar). `samples`: intercaladas, `frames × channels`.
/// Devolve o tamanho.
pub fn encode(out: &mut [u8], key: &Key, h: &Header, samples: &[f32]) -> usize {
    let n = h.frames as usize * h.channels as usize;
    let len = HEADER + n * 3;
    assert!(out.len() >= len && samples.len() >= n, "buffer de pacote pequeno");
    out[0..4].copy_from_slice(MAGIC);
    out[4] = VERSION;
    out[5] = h.kind;
    out[6] = h.channels;
    out[7] = 0;
    out[8..12].copy_from_slice(&h.session.to_le_bytes());
    out[12..16].copy_from_slice(&h.seq.to_le_bytes());
    out[16..24].copy_from_slice(&h.timestamp.to_le_bytes());
    out[24..26].copy_from_slice(&h.frames.to_le_bytes());
    out[26..28].copy_from_slice(&[0, 0]);
    for (i, s) in samples[..n].iter().enumerate() {
        out[HEADER + 3 * i..HEADER + 3 * i + 3].copy_from_slice(&to_i24(*s));
    }
    let tag = hmac(key, &[&out[..28], &out[HEADER..len]]);
    out[28..36].copy_from_slice(&tag[..8]);
    len
}

/// Lê só o cabeçalho, sem conferir a assinatura (para achar a sessão).
pub fn peek(pkt: &[u8]) -> Option<Header> {
    if pkt.len() < HEADER || &pkt[0..4] != MAGIC || pkt[4] != VERSION {
        return None;
    }
    let h = Header {
        kind: pkt[5],
        channels: pkt[6],
        session: u32::from_le_bytes(pkt[8..12].try_into().ok()?),
        seq: u32::from_le_bytes(pkt[12..16].try_into().ok()?),
        timestamp: u64::from_le_bytes(pkt[16..24].try_into().ok()?),
        frames: u16::from_le_bytes(pkt[24..26].try_into().ok()?),
    };
    let n = h.frames as usize * h.channels as usize;
    (h.channels as usize <= MAX_CHANNELS && h.frames as usize <= FRAMES * 4 && pkt.len() == HEADER + n * 3).then_some(h)
}

/// Confere a assinatura e decodifica as amostras em `out` (intercaladas).
/// Devolve o cabeçalho, ou `None` se o pacote for inválido ou forjado.
pub fn decode(pkt: &[u8], key: &Key, out: &mut [f32]) -> Option<Header> {
    let h = peek(pkt)?;
    let tag = hmac(key, &[&pkt[..28], &pkt[HEADER..]]);
    if !eq_ct(&tag[..8], &pkt[28..36]) {
        return None;
    }
    let n = h.frames as usize * h.channels as usize;
    if out.len() < n {
        return None;
    }
    for (i, o) in out[..n].iter_mut().enumerate() {
        *o = from_i24(&pkt[HEADER + 3 * i..HEADER + 3 * i + 3]);
    }
    Some(h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_bate_com_o_vetor_da_rfc_4231() {
        // RFC 4231, caso 2.
        let tag = hmac(b"Jefe", &[b"what do ya want ", b"for nothing?"]);
        let hex: String = tag.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843");
    }

    #[test]
    fn ida_e_volta_com_24_bits() {
        let key = [7u8; 16];
        let samples: Vec<f32> = (0..FRAMES * 2).map(|i| ((i as f32) * 0.01).sin() * 0.9).collect();
        let h = Header { kind: KIND_AUDIO, channels: 2, session: 42, seq: 9, timestamp: 123_456, frames: FRAMES as u16 };
        let mut pkt = [0u8; MAX_PACKET];
        let len = encode(&mut pkt, &key, &h, &samples);
        let mut out = vec![0.0f32; FRAMES * 2];
        assert_eq!(decode(&pkt[..len], &key, &mut out), Some(h));
        let err = samples.iter().zip(&out).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err < 2.0 / 8_388_607.0, "erro de quantização {err}");
    }

    #[test]
    fn pacote_alterado_ou_com_outra_chave_e_recusado() {
        let key = [1u8; 16];
        let h = Header { kind: KIND_AUDIO, channels: 1, session: 1, seq: 1, timestamp: 0, frames: 4 };
        let mut pkt = [0u8; MAX_PACKET];
        let len = encode(&mut pkt, &key, &h, &[0.1, 0.2, 0.3, 0.4]);
        let mut out = [0.0f32; 4];
        assert!(decode(&pkt[..len], &[2u8; 16], &mut out).is_none(), "outra chave");
        pkt[HEADER] ^= 1;
        assert!(decode(&pkt[..len], &key, &mut out).is_none(), "carga alterada");
        assert!(decode(&pkt[..len - 1], &key, &mut out).is_none(), "tamanho errado");
    }
}
