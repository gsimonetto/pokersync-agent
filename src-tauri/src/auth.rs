//! Login do agente contra o GoTrue do Supabase (mesma auth usada pelo
//! produto web). Dois caminhos: email/senha via password grant aqui
//! embaixo, ou Google — que não roda dentro da janela nativa do Tauri,
//! então abre no navegador do sistema (ver `lib.rs::start_google_login`)
//! e volta pelo deep link `pokersync-agent://auth`. Em nenhum dos dois a
//! senha do usuário passa por aqui além do POST direto ao GoTrue; só os
//! tokens resultantes são guardados (no keychain, ver `keychain.rs`).

use crate::config::{SUPABASE_ANON_KEY, SUPABASE_URL};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct GoTrueUser {
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GoTrueSession {
    access_token: String,
    refresh_token: String,
    user: GoTrueUser,
}

#[derive(Debug, Deserialize)]
struct GoTrueError {
    #[serde(alias = "error_description", alias = "msg")]
    message: Option<String>,
}

pub struct LoginResult {
    pub access_token: String,
    pub refresh_token: String,
    pub email: Option<String>,
}

pub async fn login_with_password(email: &str, password: &str) -> Result<LoginResult, String> {
    let client = reqwest::Client::new();
    let url = format!("{SUPABASE_URL}/auth/v1/token?grant_type=password");
    let resp = client
        .post(url)
        .header("apikey", SUPABASE_ANON_KEY)
        .json(&serde_json::json!({ "email": email, "password": password }))
        .send()
        .await
        .map_err(|e| format!("Falha de rede ao autenticar: {e}"))?;

    let status = resp.status();
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;

    if !status.is_success() {
        let msg = serde_json::from_slice::<GoTrueError>(&bytes)
            .ok()
            .and_then(|e| e.message)
            .unwrap_or_else(|| "Email ou senha inválidos.".to_string());
        return Err(msg);
    }

    let session: GoTrueSession = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    Ok(LoginResult {
        access_token: session.access_token,
        refresh_token: session.refresh_token,
        email: session.user.email,
    })
}

/// Troca o refresh_token por um access_token novo — chamado quando o
/// backend responde 401 durante um sync (token expirado).
pub async fn refresh_session(refresh_token: &str) -> Result<LoginResult, String> {
    let client = reqwest::Client::new();
    let url = format!("{SUPABASE_URL}/auth/v1/token?grant_type=refresh_token");
    let resp = client
        .post(url)
        .header("apikey", SUPABASE_ANON_KEY)
        .json(&serde_json::json!({ "refresh_token": refresh_token }))
        .send()
        .await
        .map_err(|e| format!("Falha de rede ao renovar sessão: {e}"))?;

    let status = resp.status();
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err("Sessão expirada — faça login novamente.".to_string());
    }
    let session: GoTrueSession = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    Ok(LoginResult {
        access_token: session.access_token,
        refresh_token: session.refresh_token,
        email: session.user.email,
    })
}

/// O deep link de volta do login com Google (ver `lib.rs`) só traz os
/// tokens — busca o email aqui pra exibir "Conectado como ..." na UI,
/// igual ao fluxo de email/senha.
pub async fn fetch_user_email(access_token: &str) -> Option<String> {
    let client = reqwest::Client::new();
    let url = format!("{SUPABASE_URL}/auth/v1/user");
    let resp = client
        .get(url)
        .header("apikey", SUPABASE_ANON_KEY)
        .bearer_auth(access_token)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json::<GoTrueUser>().await.ok()?.email
}
