//! Login do agente contra o GoTrue do Supabase (mesma auth usada pelo
//! produto web). Dois caminhos: email/senha via password grant aqui
//! embaixo, ou Google — que não roda dentro da janela nativa do Tauri,
//! então abre no navegador do sistema (ver `lib.rs::start_google_login`)
//! e volta pelo deep link `radar-pokersync://auth`. Em nenhum dos dois a
//! senha do usuário passa por aqui além do POST direto ao GoTrue; só os
//! tokens resultantes são guardados (no keychain, ver `keychain.rs`).

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
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

/// O access_token é um JWT — o claim `email` já vem embutido nele
/// (assinado pelo GoTrue no momento em que o Google devolveu o login), só
/// decodificar a parte do meio em base64. Evita uma chamada de rede a
/// mais depois do deep link (que podia falhar — proxy, DNS, o que for —
/// sem aparecer erro nenhum pro jogador, só deixando "Conectado como"
/// em branco).
fn decode_email_from_jwt(access_token: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Claims {
        email: Option<String>,
    }
    let payload_b64 = access_token.split('.').nth(1)?;
    let payload = URL_SAFE_NO_PAD.decode(payload_b64).ok()?;
    serde_json::from_slice::<Claims>(&payload).ok()?.email
}

/// O deep link de volta do login com Google (ver `lib.rs`) só traz os
/// tokens — busca o email aqui pra exibir "Conectado como ..." na UI,
/// igual ao fluxo de email/senha. Tenta primeiro decodificar do próprio
/// token (rápido, sem rede); só bate no GoTrue se por algum motivo o
/// token não tiver o claim (não deveria acontecer com os tokens que o
/// Supabase emite hoje, mas mais vale ter o caminho de volta).
pub async fn fetch_user_email(access_token: &str) -> Option<String> {
    if let Some(email) = decode_email_from_jwt(access_token) {
        return Some(email);
    }

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

#[cfg(test)]
mod tests {
    use super::decode_email_from_jwt;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};

    fn fake_jwt(payload_json: &str) -> String {
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"HS256\"}");
        let payload = URL_SAFE_NO_PAD.encode(payload_json.as_bytes());
        format!("{header}.{payload}.fake-signature")
    }

    #[test]
    fn decodes_email_from_real_shaped_token() {
        let token = fake_jwt(r#"{"sub":"123","email":"jogador@pokersync.com.br","role":"authenticated"}"#);
        assert_eq!(decode_email_from_jwt(&token).as_deref(), Some("jogador@pokersync.com.br"));
    }

    #[test]
    fn returns_none_when_claim_missing() {
        let token = fake_jwt(r#"{"sub":"123","role":"authenticated"}"#);
        assert_eq!(decode_email_from_jwt(&token), None);
    }

    #[test]
    fn returns_none_for_garbage_input() {
        assert_eq!(decode_email_from_jwt("nao-e-um-jwt"), None);
    }
}
