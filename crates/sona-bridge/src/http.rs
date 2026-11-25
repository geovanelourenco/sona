//! Cliente HTTP mínimo (LAN, sem TLS) para o login no servidor.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// POST JSON; devolve (status, corpo).
pub fn post_json(addr: &str, path: &str, body: &str) -> Result<(u16, String), String> {
    let mut s = TcpStream::connect(addr).map_err(|e| format!("Servidor {addr} não respondeu: {e}"))?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&buf).to_string();
    let (head, rest) = text.split_once("\r\n\r\n").ok_or("Resposta inválida do servidor.")?;
    let status: u16 = head.split_whitespace().nth(1).and_then(|c| c.parse().ok()).ok_or("Resposta inválida do servidor.")?;
    // Corpo com "Transfer-Encoding: chunked": junta os pedaços.
    let body = if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        let mut out = String::new();
        let mut r = rest;
        while let Some((size, tail)) = r.split_once("\r\n") {
            let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
            if n == 0 || tail.len() < n {
                break;
            }
            out.push_str(&tail[..n]);
            r = tail[n..].trim_start_matches("\r\n");
        }
        out
    } else {
        rest.to_string()
    };
    Ok((status, body))
}
