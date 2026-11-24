//! Autenticação por token com perfis (admin, operador, monitor).
//!
//! Tokens ficam só como hash SHA-256 no disco. No primeiro início o servidor
//! cria um token de admin e mostra uma vez no log.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Admin,
    Operator,
    Monitor,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenEntry {
    pub name: String,
    pub role: Role,
    pub hash: String,
}

#[derive(Default, Serialize, Deserialize)]
pub struct AuthFile {
    pub tokens: Vec<TokenEntry>,
}

pub struct Auth {
    file: AuthFile,
    path: PathBuf,
}

pub fn new_token() -> String {
    let mut b = [0u8; 18];
    getrandom::fill(&mut b).expect("gerador aleatório do sistema");
    hex::encode(b)
}

fn hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// Comparação em tempo constante.
fn eq_ct(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl Auth {
    /// Carrega o arquivo. Devolve o token de admin quando acabou de criar.
    pub fn load(path: &Path) -> (Auth, Option<String>) {
        let file: AuthFile = std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let mut auth = Auth { file, path: path.to_path_buf() };
        if auth.file.tokens.iter().any(|t| t.role == Role::Admin) {
            return (auth, None);
        }
        let token = auth.issue("admin", Role::Admin);
        (auth, Some(token))
    }

    pub fn issue(&mut self, name: &str, role: Role) -> String {
        let token = new_token();
        self.file.tokens.push(TokenEntry { name: name.into(), role, hash: hash(&token) });
        self.persist();
        token
    }

    pub fn verify(&self, token: &str) -> Option<(String, Role)> {
        let h = hash(token);
        self.file.tokens.iter().find(|t| eq_ct(&t.hash, &h)).map(|t| (t.name.clone(), t.role))
    }

    fn persist(&self) {
        let bytes = serde_json::to_vec_pretty(&self.file).expect("serializa");
        if let Err(e) = crate::state::write_atomic(&self.path, &bytes) {
            tracing::error!("Falha ao salvar tokens: {e}");
        }
    }
}
