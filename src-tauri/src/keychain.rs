//! Tokens de sessão (access + refresh) guardados no keychain nativo do SO
//! — Windows Credential Manager, macOS Keychain, Secret Service no Linux —
//! em vez de arquivo texto plano. O resto da config (URL, device, pastas)
//! não é segredo e continua em `config.rs`.

use keyring::Entry;
use serde::{Deserialize, Serialize};

const SERVICE: &str = "com.pokersync.agent";
const ACCOUNT: &str = "session";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
}

fn entry() -> Result<Entry, String> {
    Entry::new(SERVICE, ACCOUNT).map_err(|e| format!("Keychain indisponível: {e}"))
}

/// `None` tanto quando não há sessão salva quanto quando o backend do
/// keychain falha (ex.: ambiente sem Secret Service no Linux) — nesse caso
/// o app trata como "não logado" e pede login de novo, em vez de travar.
pub fn load() -> Option<Tokens> {
    let raw = entry().ok()?.get_password().ok()?;
    serde_json::from_str(&raw).ok()
}

pub fn save(tokens: &Tokens) -> Result<(), String> {
    let raw = serde_json::to_string(tokens).map_err(|e| e.to_string())?;
    entry()?.set_password(&raw).map_err(|e| e.to_string())
}

pub fn clear() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
