//! Config local do agente: URL do produto, sessão Supabase e pastas extras
//! por sala. Persistida em JSON em app_config_dir() (por usuário/SO, via
//! tauri::Manager::path). V1 propositalmente simples: tokens em texto
//! plano no disco do próprio usuário — aceitável pra uma sessão de agente
//! local, mas migrar pro keychain do SO é o próximo passo óbvio (ver
//! README) antes de tratar isso como produção.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

// Mesmos valores públicos usados pelo client web (lib/supabase/client.ts) —
// a anon key é destinada a ficar em qualquer bundle de cliente, RLS é quem
// protege os dados.
pub const SUPABASE_URL: &str = "https://olgziujndtlvxegcnaoq.supabase.co";
pub const SUPABASE_ANON_KEY: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6Im9sZ3ppdWpuZHRsdnhlZ2NuYW9xIiwicm9sZSI6ImFub24iLCJpYXQiOjE3ODUxNjExMDYsImV4cCI6MjEwMDczNzEwNn0.NspOVPJcZ_pjodnDTHzalDIcjVkqoR6YVfwiN4MpBbY";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// URL do produto PokerSync (onde /api/agent/* responde). Sem default
    /// de produção porque não é conhecida deste crate — configurada na UI.
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub user_email: Option<String>,
    /// Identifica esta instalação em hand_sync_devices.device_id — gerado
    /// uma vez e reaproveitado entre sessões do app.
    #[serde(default = "new_device_id")]
    pub device_id: String,
    #[serde(default = "default_device_name")]
    pub device_name: String,
    /// Pastas adicionais escolhidas manualmente pelo usuário, por sala
    /// (slug de PokerRoom) — somam-se aos caminhos padrão do SO.
    #[serde(default)]
    pub extra_folders: HashMap<String, Vec<String>>,
}

fn new_device_id() -> String {
    format!("agent-{}", uuid::Uuid::new_v4())
}

fn default_device_name() -> String {
    hostname()
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "PC desconhecido".to_string())
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            access_token: None,
            refresh_token: None,
            user_email: None,
            device_id: new_device_id(),
            device_name: default_device_name(),
            extra_folders: HashMap::new(),
        }
    }
}

impl AppConfig {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self).unwrap_or_default())
    }

    pub fn is_logged_in(&self) -> bool {
        self.access_token.is_some()
    }
}
