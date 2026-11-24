//! Acesso ao Sona. Não há perfis: quem entra tem acesso total.
//!
//! - Neste computador (loopback): entra direto, ou com a "senha do app" se ela
//!   estiver definida.
//! - Pela rede: só com a "senha de acesso remoto". Sem ela, o acesso remoto fica
//!   desligado e o servidor recusa pedidos que não vêm deste computador.
//!
//! Senhas ficam como hash Argon2id com salt. Depois de entrar, o navegador usa um
//! token de sessão (hash SHA-256 no disco). Trocar ou apagar uma senha encerra as
//! sessões abertas com ela, menos a de quem fez a troca (tirar a senha do app
//! não encerra nada). O arquivo é relido
//! quando muda, então a recuperação pela linha de comando vale sem reiniciar.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Sessão aberta neste computador.
    Local,
    /// Sessão aberta de outro computador da rede.
    Remote,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenEntry {
    pub name: String,
    pub scope: Scope,
    pub hash: String,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthFile {
    pub tokens: Vec<TokenEntry>,
    /// Hash "argon2id$<salt>$<hash>" da senha do app (neste computador).
    pub app_password: Option<String>,
    /// Hash da senha de acesso remoto. Ausente = acesso remoto desligado.
    pub remote_password: Option<String>,
    /// A tela de primeira configuração já foi concluída.
    pub setup_done: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Status {
    pub app_password: bool,
    pub remote_enabled: bool,
    pub setup_done: bool,
}

pub struct Auth {
    file: AuthFile,
    path: PathBuf,
    mtime: Option<SystemTime>,
}

pub const MIN_PASSWORD: usize = 6;
/// Sessões guardadas. As mais antigas saem primeiro.
const MAX_TOKENS: usize = 100;

pub fn new_token() -> String {
    let mut b = [0u8; 18];
    getrandom::fill(&mut b).expect("gerador aleatório do sistema");
    hex::encode(b)
}

fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// Comparação em tempo constante.
fn eq_ct(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn argon(password: &str, salt: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    argon2::Argon2::default().hash_password_into(password.as_bytes(), salt, &mut out).expect("parâmetros fixos do Argon2");
    out
}

pub fn hash_password(password: &str) -> String {
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).expect("gerador aleatório do sistema");
    format!("argon2id${}${}", hex::encode(salt), hex::encode(argon(password, &salt)))
}

pub fn verify_password(password: &str, stored: &str) -> bool {
    let mut parts = stored.split('$');
    let (Some("argon2id"), Some(salt), Some(hash)) = (parts.next(), parts.next(), parts.next()) else { return false };
    let (Ok(salt), Ok(hash)) = (hex::decode(salt), hex::decode(hash)) else { return false };
    eq_ct(&argon(password, &salt), &hash)
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl Auth {
    pub fn load(path: &Path) -> Auth {
        let file: AuthFile = std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Auth { file, path: path.to_path_buf(), mtime: mtime(path) }
    }

    /// Relê o arquivo se outro processo (a linha de comando) mudou.
    fn refresh(&mut self) {
        let now = mtime(&self.path);
        if now.is_some() && now != self.mtime {
            if let Some(file) = std::fs::read(&self.path).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
                self.file = file;
            }
            self.mtime = now;
        }
    }

    pub fn status(&mut self) -> Status {
        self.refresh();
        Status {
            app_password: self.file.app_password.is_some(),
            remote_enabled: self.file.remote_password.is_some(),
            setup_done: self.file.setup_done,
        }
    }

    /// Confere a senha do escopo. Local sem senha do app: entra direto.
    /// Remoto sem senha remota: nunca entra.
    pub fn check(&mut self, scope: Scope, password: &str) -> bool {
        self.refresh();
        match scope {
            Scope::Local => self.file.app_password.as_deref().is_none_or(|h| verify_password(password, h)),
            Scope::Remote => self.file.remote_password.as_deref().is_some_and(|h| verify_password(password, h)),
        }
    }

    pub fn issue(&mut self, name: &str, scope: Scope) -> String {
        self.refresh();
        let token = new_token();
        self.file.tokens.push(TokenEntry { name: name.into(), scope, hash: hash_token(&token) });
        let extra = self.file.tokens.len().saturating_sub(MAX_TOKENS);
        self.file.tokens.drain(..extra);
        self.persist();
        token
    }

    pub fn verify(&mut self, token: &str) -> Option<(String, Scope)> {
        self.refresh();
        let h = hash_token(token);
        self.file.tokens.iter().find(|t| eq_ct(t.hash.as_bytes(), h.as_bytes())).map(|t| (t.name.clone(), t.scope))
    }

    /// Define (Some) ou apaga (None) a senha do escopo. Criar ou trocar encerra
    /// as sessões desse escopo, menos a de `keep` (quem fez a troca). Desligar o
    /// acesso remoto encerra todas as remotas.
    pub fn set_password(&mut self, scope: Scope, password: Option<&str>, keep: Option<&str>) -> Result<(), String> {
        self.refresh();
        if let Some(p) = password {
            if p.chars().count() < MIN_PASSWORD {
                return Err(format!("A senha precisa de pelo menos {MIN_PASSWORD} caracteres."));
            }
        }
        let hash = password.map(hash_password);
        match scope {
            Scope::Local => self.file.app_password = hash,
            Scope::Remote => self.file.remote_password = hash,
        }
        // Desligar o acesso remoto derruba todas as sessões remotas, inclusive a
        // de quem desligou.
        // Tirar a senha do app não encerra sessões (nada fica menos protegido do
        // que já estava aberto).
        let keep = if scope == Scope::Remote && password.is_none() { None } else { keep.map(hash_token) };
        if !(scope == Scope::Local && password.is_none()) {
            self.file.tokens.retain(|t| t.scope != scope || keep.as_deref() == Some(t.hash.as_str()));
        }
        self.persist();
        Ok(())
    }

    pub fn finish_setup(&mut self) {
        self.refresh();
        if !self.file.setup_done {
            self.file.setup_done = true;
            self.persist();
        }
    }

    fn persist(&mut self) {
        let bytes = serde_json::to_vec_pretty(&self.file).expect("serializa");
        if let Err(e) = crate::state::write_atomic(&self.path, &bytes) {
            tracing::error!("Falha ao salvar acessos: {e}");
        }
        self.mtime = mtime(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("sona-auth-{}", new_token()));
        std::fs::create_dir_all(&d).unwrap();
        d.join("auth.json")
    }

    #[test]
    fn sem_senha_do_app_entra_direto_neste_computador() {
        let mut a = Auth::load(&tmp());
        assert!(a.check(Scope::Local, ""));
        assert!(!a.check(Scope::Remote, ""), "remoto desligado sem senha remota");
        assert!(!a.status().setup_done);
    }

    #[test]
    fn senhas_conferem_e_nao_ficam_em_claro() {
        let p = tmp();
        let mut a = Auth::load(&p);
        a.set_password(Scope::Remote, Some("louvor2026"), None).unwrap();
        a.set_password(Scope::Local, Some("so-aqui-123"), None).unwrap();
        assert!(a.check(Scope::Remote, "louvor2026"));
        assert!(!a.check(Scope::Remote, "so-aqui-123"), "cada senha vale no seu escopo");
        assert!(a.check(Scope::Local, "so-aqui-123"));
        assert!(!a.check(Scope::Local, ""));
        let disk = std::fs::read_to_string(&p).unwrap();
        assert!(!disk.contains("louvor2026") && !disk.contains("so-aqui-123"));
        assert!(a.set_password(Scope::Remote, Some("123"), None).is_err());
    }

    #[test]
    fn trocar_senha_encerra_sessoes_menos_a_de_quem_trocou() {
        let mut a = Auth::load(&tmp());
        a.set_password(Scope::Remote, Some("primeira"), None).unwrap();
        let eu = a.issue("Ana", Scope::Remote);
        let outro = a.issue("Bia", Scope::Remote);
        let local = a.issue("Este computador", Scope::Local);
        a.set_password(Scope::Remote, Some("segunda"), Some(&eu)).unwrap();
        assert!(a.verify(&eu).is_some());
        assert!(a.verify(&outro).is_none());
        assert!(a.verify(&local).is_some(), "sessão local não depende da senha remota");
        a.set_password(Scope::Remote, None, None).unwrap();
        assert!(!a.status().remote_enabled);
        assert!(a.verify(&eu).is_none(), "desligar o remoto derruba todas as sessões remotas");
    }

    #[test]
    fn mudanca_feita_por_outro_processo_vale_sem_reiniciar() {
        let p = tmp();
        let mut servidor = Auth::load(&p);
        servidor.set_password(Scope::Local, Some("esqueci-essa"), None).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let mut cli = Auth::load(&p);
        cli.set_password(Scope::Local, None, None).unwrap();
        assert!(servidor.check(Scope::Local, ""), "recuperação pela linha de comando");
    }
}
