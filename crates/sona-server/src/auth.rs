//! Autenticação: senha por perfil (para o acesso pela rede) e tokens.
//!
//! - Senha: definida na máquina do servidor (`sona-server password`) ou por um
//!   admin no console. Fica só como hash Argon2id com salt no disco.
//! - Token: o navegador recebe um token de sessão depois de entrar com a senha.
//!   O token de admin criado no primeiro início continua valendo na máquina.
//!   Trocar ou apagar a senha de um perfil derruba as sessões abertas com ela.
//!
//! O arquivo é relido quando muda no disco, então a senha trocada pela linha de
//! comando vale sem reiniciar o servidor.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Admin,
    Operator,
    Monitor,
}

impl Role {
    pub const ALL: [Role; 3] = [Role::Admin, Role::Operator, Role::Monitor];
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenEntry {
    pub name: String,
    pub role: Role,
    pub hash: String,
    /// Sessão aberta com senha: cai quando a senha do perfil muda.
    #[serde(default)]
    pub via_password: bool,
}

#[derive(Default, Serialize, Deserialize)]
pub struct AuthFile {
    pub tokens: Vec<TokenEntry>,
    /// Hash Argon2id por perfil: "argon2id$<salt hex>$<hash hex>".
    #[serde(default)]
    pub passwords: BTreeMap<Role, String>,
}

pub struct Auth {
    file: AuthFile,
    path: PathBuf,
    mtime: Option<SystemTime>,
}

pub const MIN_PASSWORD: usize = 6;

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
    /// Carrega o arquivo. Devolve o token de admin quando acabou de criar.
    pub fn load(path: &Path) -> (Auth, Option<String>) {
        let file: AuthFile = std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let mut auth = Auth { file, path: path.to_path_buf(), mtime: mtime(path) };
        if auth.file.tokens.iter().any(|t| t.role == Role::Admin && !t.via_password) {
            return (auth, None);
        }
        let token = auth.issue("admin", Role::Admin, false);
        (auth, Some(token))
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

    pub fn issue(&mut self, name: &str, role: Role, via_password: bool) -> String {
        self.refresh();
        let token = new_token();
        self.file.tokens.push(TokenEntry { name: name.into(), role, hash: hash_token(&token), via_password });
        self.persist();
        token
    }

    pub fn verify(&mut self, token: &str) -> Option<(String, Role)> {
        self.refresh();
        let h = hash_token(token);
        self.file.tokens.iter().find(|t| eq_ct(t.hash.as_bytes(), h.as_bytes())).map(|t| (t.name.clone(), t.role))
    }

    /// Perfil da senha, testando do mais alto para o mais baixo.
    pub fn check_password(&mut self, password: &str) -> Option<Role> {
        self.refresh();
        Role::ALL.into_iter().find(|r| self.file.passwords.get(r).is_some_and(|h| verify_password(password, h)))
    }

    /// Define (Some) ou apaga (None) a senha de um perfil. Derruba as sessões
    /// abertas com a senha antiga desse perfil.
    pub fn set_password(&mut self, role: Role, password: Option<&str>) -> Result<(), String> {
        self.refresh();
        match password {
            Some(p) if p.chars().count() < MIN_PASSWORD => return Err(format!("A senha precisa de pelo menos {MIN_PASSWORD} caracteres.")),
            Some(p) => {
                if let Some((other, _)) = self.file.passwords.iter().find(|(r, h)| **r != role && verify_password(p, h)) {
                    return Err(format!("Essa senha já é a do perfil {other:?}. Use senhas diferentes por perfil."));
                }
                self.file.passwords.insert(role, hash_password(p));
            }
            None => {
                self.file.passwords.remove(&role);
            }
        }
        self.file.tokens.retain(|t| !(t.via_password && t.role == role));
        self.persist();
        Ok(())
    }

    pub fn has_password(&mut self) -> bool {
        self.refresh();
        !self.file.passwords.is_empty()
    }

    pub fn password_roles(&mut self) -> Vec<Role> {
        self.refresh();
        self.file.passwords.keys().copied().collect()
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
    fn senha_confere_e_nao_fica_em_claro() {
        let p = tmp();
        let (mut a, _) = Auth::load(&p);
        a.set_password(Role::Operator, Some("louvor2026")).unwrap();
        assert_eq!(a.check_password("louvor2026"), Some(Role::Operator));
        assert_eq!(a.check_password("louvor2025"), None);
        assert!(!std::fs::read_to_string(&p).unwrap().contains("louvor2026"));
    }

    #[test]
    fn senha_curta_ou_repetida_e_recusada() {
        let (mut a, _) = Auth::load(&tmp());
        assert!(a.set_password(Role::Operator, Some("123")).is_err());
        a.set_password(Role::Admin, Some("segredo-admin")).unwrap();
        assert!(a.set_password(Role::Operator, Some("segredo-admin")).is_err());
    }

    #[test]
    fn trocar_senha_derruba_sessoes_do_perfil() {
        let (mut a, admin) = Auth::load(&tmp());
        a.set_password(Role::Operator, Some("primeira")).unwrap();
        let s = a.issue("Ana", Role::Operator, true);
        assert!(a.verify(&s).is_some());
        a.set_password(Role::Operator, Some("segunda")).unwrap();
        assert!(a.verify(&s).is_none(), "sessão antiga caiu");
        assert!(a.verify(&admin.unwrap()).is_some(), "token de admin da máquina continua");
    }

    #[test]
    fn mudanca_feita_por_outro_processo_vale_sem_reiniciar() {
        let p = tmp();
        let (mut servidor, _) = Auth::load(&p);
        assert!(!servidor.has_password());
        std::thread::sleep(std::time::Duration::from_millis(20));
        let (mut cli, _) = Auth::load(&p);
        cli.set_password(Role::Operator, Some("pela-linha")).unwrap();
        assert_eq!(servidor.check_password("pela-linha"), Some(Role::Operator));
    }
}
