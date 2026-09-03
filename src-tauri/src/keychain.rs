//! Tokens de sessão (access + refresh) guardados no keychain nativo do SO
//! — Windows Credential Manager, macOS Keychain, Secret Service no Linux —
//! em vez de arquivo texto plano. O resto da config (URL, device, pastas)
//! não é segredo e continua em `config.rs`.

use keyring::Entry;

const SERVICE: &str = "com.pokersync.agent";
// Guardados em DUAS entradas separadas, não uma só com {access,refresh}
// serializado em JSON — o Windows Credential Manager tem um limite rígido
// de 2560 caracteres (UTF-16) por credencial (CRED_MAX_CREDENTIAL_BLOB_SIZE),
// e o access_token (JWT do Supabase, com claims de usuário) somado ao
// refresh_token e ao overhead do JSON estourava esse limite na prática
// (erro relatado: "Attribute 'password encoded as UTF-16' is longer than
// platform limit of 2560 chars" ao logar com Google). Cada token sozinho
// fica bem abaixo do limite.
const ACCESS_ACCOUNT: &str = "session-access";
const REFRESH_ACCOUNT: &str = "session-refresh";

#[derive(Debug, Clone)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
}

fn entry(account: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, account).map_err(|e| format!("Keychain indisponível: {e}"))
}

/// `None` tanto quando não há sessão salva quanto quando o backend do
/// keychain falha (ex.: ambiente sem Secret Service no Linux) — nesse caso
/// o app trata como "não logado" e pede login de novo, em vez de travar.
pub fn load() -> Option<Tokens> {
    let access_token = entry(ACCESS_ACCOUNT).ok()?.get_password().ok()?;
    let refresh_token = entry(REFRESH_ACCOUNT).ok()?.get_password().ok()?;
    Some(Tokens { access_token, refresh_token })
}

pub fn save(tokens: &Tokens) -> Result<(), String> {
    entry(ACCESS_ACCOUNT)?
        .set_password(&tokens.access_token)
        .map_err(|e| e.to_string())?;
    entry(REFRESH_ACCOUNT)?
        .set_password(&tokens.refresh_token)
        .map_err(|e| e.to_string())
}

pub fn clear() -> Result<(), String> {
    for account in [ACCESS_ACCOUNT, REFRESH_ACCOUNT] {
        match entry(account)?.delete_credential() {
            Ok(()) => {}
            Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}
