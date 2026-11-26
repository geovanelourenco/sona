//! Descoberta na rede local por mDNS/DNS-SD ("Bonjour"), como o Lumo faz.
//!
//! - O servidor anuncia o serviço `_sona._tcp` e o nome `sona.local`.
//! - O bridge procura o serviço e lista os servidores encontrados.
//!
//! O nome `sona.local` permite abrir `http://sona.local:7340` no navegador
//! (Windows 10/11, macOS e Linux com Avahi resolvem nomes `.local`).

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::time::{Duration, Instant};

pub const SERVICE: &str = "_sona._tcp.local.";
pub const HOSTNAME: &str = "sona.local.";

/// Nome deste computador (para o nome da instância no anúncio).
pub fn computer_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "servidor".into())
}

/// Anúncio ativo; parar = soltar.
pub struct Announcement {
    daemon: ServiceDaemon,
    fullname: String,
}

impl Drop for Announcement {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}

/// Anuncia o servidor. Endereços acompanham as interfaces sozinhos.
pub fn announce(port: u16, version: &str) -> Result<Announcement, String> {
    let daemon = ServiceDaemon::new().map_err(|e| format!("mDNS indisponível: {e}"))?;
    let instance = format!("Sona ({})", computer_name());
    let props = [("versao", version), ("caminho", "/")];
    let info = ServiceInfo::new(SERVICE, &instance, HOSTNAME, "", port, &props[..]).map_err(|e| e.to_string())?.enable_addr_auto();
    let fullname = info.get_fullname().to_string();
    daemon.register(info).map_err(|e| format!("mDNS: {e}"))?;
    Ok(Announcement { daemon, fullname })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// Nome da instância, ex.: "Sona (PC-SOM)".
    pub name: String,
    /// Endereço para conectar, ex.: "192.168.0.10:7340" (IPv4 preferido).
    pub address: String,
    /// Todos os IPv4 anunciados.
    pub ips: Vec<String>,
    pub port: u16,
}

fn local_ipv4() -> Vec<std::net::Ipv4Addr> {
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|i| match i.ip() {
            std::net::IpAddr::V4(v) => Some(v),
            _ => None,
        })
        .collect()
}

/// Procura servidores por até `wait`. Bloqueante.
pub fn discover(wait: Duration) -> Vec<Found> {
    let Ok(daemon) = ServiceDaemon::new() else { return Vec::new() };
    let Ok(rx) = daemon.browse(SERVICE) else { return Vec::new() };
    let mut found: Vec<Found> = Vec::new();
    let t0 = Instant::now();
    while let Some(left) = wait.checked_sub(t0.elapsed()) {
        match rx.recv_timeout(left) {
            Ok(ServiceEvent::ServiceResolved(s)) => {
                let v4 = s.get_addresses_v4();
                let mut ips: Vec<String> = v4.iter().filter(|a| !a.is_loopback()).map(|a| a.to_string()).collect();
                ips.sort();
                // Servidor neste computador: 127.0.0.1 (vale a regra de acesso local).
                // Noutro computador: o primeiro IP da rede, nunca o loopback.
                let local = local_ipv4();
                let same_host = v4.iter().any(|a| a.is_loopback() || local.contains(a));
                let host = if same_host {
                    "127.0.0.1".to_string()
                } else if let Some(ip) = ips.first() {
                    ip.clone()
                } else {
                    continue;
                };
                let name = s.fullname.trim_end_matches(SERVICE).trim_end_matches('.').replace("\\032", " ").replace('\\', "");
                let item = Found { name, address: format!("{host}:{}", s.port), ips, port: s.port };
                match found.iter_mut().find(|f| f.name == item.name) {
                    Some(f) => *f = item,
                    None => found.push(item),
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    let _ = daemon.stop_browse(SERVICE);
    let _ = daemon.shutdown();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// Os testes anunciam o mesmo nome (`sona.local`): um de cada vez.
#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anuncio_e_achado_na_rede_local() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let port = 47_000 + (std::process::id() % 1000) as u16;
        let _a = announce(port, "teste").expect("anúncio");
        let found = discover(Duration::from_secs(4));
        let me = found.iter().find(|f| f.port == port);
        assert!(me.is_some(), "servidor não achado: {found:?}");
        let me = me.unwrap();
        assert!(me.name.starts_with("Sona ("), "{me:?}");
        assert!(me.address.ends_with(&format!(":{port}")));
    }
}

#[cfg(test)]
mod hostname_tests {
    use super::*;
    use mdns_sd::HostnameResolutionEvent;

    /// `sona.local` responde com um endereço (é o que o navegador consulta).
    #[test]
    fn nome_sona_local_responde() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let port = 48_000 + (std::process::id() % 1000) as u16;
        let _a = announce(port, "teste").expect("anúncio");
        std::thread::sleep(Duration::from_millis(500));
        let d = ServiceDaemon::new().unwrap();
        let rx = d.resolve_hostname(HOSTNAME, Some(4000)).unwrap();
        let mut ok = false;
        while let Ok(ev) = rx.recv_timeout(Duration::from_secs(5)) {
            match ev {
                HostnameResolutionEvent::AddressesFound(_, addrs) if !addrs.is_empty() => {
                    ok = true;
                    break;
                }
                HostnameResolutionEvent::SearchTimeout(_) => break,
                _ => {}
            }
        }
        let _ = d.shutdown();
        assert!(ok, "sona.local não respondeu");
    }
}
