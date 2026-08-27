mod auth;
mod config;
mod keychain;

use config::AppConfig;
use keychain::Tokens;
use scanner::{discover_files, read_pending, PokerRoom, SyncState};
use std::path::PathBuf;
use std::sync::Mutex;
use sync_client::{chunk_files, DeviceInfo, SyncClient, SyncFile, DEFAULT_BATCH_SIZE};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_opener::OpenerExt;

struct AppState {
    config_path: PathBuf,
    state_dir: PathBuf,
    config: Mutex<AppConfig>,
    /// Nonce do login com Google em andamento (gerado em
    /// `start_google_login`, conferido quando o deep link volta) —
    /// protege contra um deep link de origem estranha ser aceito como se
    /// fosse resposta de um login que o agente pediu.
    pending_google_state: Mutex<Option<String>>,
}

#[derive(serde::Serialize)]
struct ConfigDto {
    base_url: String,
    logged_in: bool,
    user_email: Option<String>,
    device_name: String,
    extra_folders: std::collections::HashMap<String, Vec<String>>,
}

fn config_dto(c: &AppConfig) -> ConfigDto {
    ConfigDto {
        base_url: c.base_url.clone(),
        logged_in: keychain::load().is_some(),
        user_email: c.user_email.clone(),
        device_name: c.device_name.clone(),
        extra_folders: c.extra_folders.clone(),
    }
}

#[tauri::command]
fn get_config(state: State<AppState>) -> ConfigDto {
    config_dto(&state.config.lock().unwrap())
}

#[tauri::command]
fn save_base_url(state: State<AppState>, base_url: String) -> Result<(), String> {
    let mut cfg = state.config.lock().unwrap();
    cfg.base_url = normalize_base_url(&base_url);
    cfg.save(&state.config_path).map_err(|e| e.to_string())
}

/// Usuário digita "www.pokersync.com.br", ou até "pokersync.com.br/" —
/// sem "https://" na frente a URL não é absoluta e o reqwest recusa
/// montar a requisição ("builder error" na UI, sem explicar o motivo).
/// Aceitamos o que o usuário digitar e completamos o esquema.
fn normalize_base_url(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    }
}

#[tauri::command]
fn save_device_name(state: State<AppState>, device_name: String) -> Result<(), String> {
    let mut cfg = state.config.lock().unwrap();
    cfg.device_name = device_name;
    cfg.save(&state.config_path).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_extra_folders(
    state: State<AppState>,
    room: String,
    folders: Vec<String>,
) -> Result<(), String> {
    let mut cfg = state.config.lock().unwrap();
    cfg.extra_folders.insert(room, folders);
    cfg.save(&state.config_path).map_err(|e| e.to_string())
}

#[tauri::command]
async fn login(
    state: State<'_, AppState>,
    email: String,
    password: String,
) -> Result<ConfigDto, String> {
    let result = auth::login_with_password(&email, &password).await?;
    keychain::save(&Tokens {
        access_token: result.access_token,
        refresh_token: result.refresh_token,
    })?;
    let mut cfg = state.config.lock().unwrap();
    cfg.user_email = result.email;
    cfg.save(&state.config_path).map_err(|e| e.to_string())?;
    Ok(config_dto(&cfg))
}

/// Abre o navegador do sistema na tela de login do agente (Google não
/// funciona dentro da webview embutida). O resultado volta assíncrono,
/// pelo deep link `pokersync-agent://auth` — ver `handle_deep_link`.
#[tauri::command]
fn start_google_login(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    use rand::Rng;
    let nonce: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();
    *state.pending_google_state.lock().unwrap() = Some(nonce.clone());

    let (base_url, device_name) = {
        let cfg = state.config.lock().unwrap();
        (cfg.base_url.clone(), cfg.device_name.clone())
    };
    let mut url = url::Url::parse(&format!("{base_url}/agent-login"))
        .map_err(|e| format!("URL do PokerSync inválida: {e}"))?;
    url.query_pairs_mut()
        .append_pair("state", &nonce)
        .append_pair("device", &device_name);

    app.opener()
        .open_url(url.to_string(), None::<&str>)
        .map_err(|e| format!("Não consegui abrir o navegador: {e}"))
}

/// Chamado pelo handler de deep link (`run()`) quando
/// `pokersync-agent://auth?...` volta do login com Google. Confere o
/// nonce, resolve o email do token e salva a sessão — mesmo destino final
/// de `login()` (email/senha), só que assíncrono e sem senha nenhuma
/// passando pelo agente.
async fn complete_google_login(app: AppHandle, access_token: String, refresh_token: String, received_state: String) {
    let state = app.state::<AppState>();
    let state_matches = {
        let mut pending = state.pending_google_state.lock().unwrap();
        let matches = pending.as_deref() == Some(received_state.as_str()) && !received_state.is_empty();
        if matches {
            *pending = None;
        }
        matches
    };
    if !state_matches {
        let _ = app.emit(
            "google-login-result",
            serde_json::json!({ "ok": false, "error": "Login não corresponde ao que o agente pediu — tente de novo." }),
        );
        return;
    }

    let email = auth::fetch_user_email(&access_token).await;

    if let Err(e) = keychain::save(&Tokens { access_token, refresh_token }) {
        let _ = app.emit("google-login-result", serde_json::json!({ "ok": false, "error": e }));
        return;
    }

    {
        let mut cfg = state.config.lock().unwrap();
        cfg.user_email = email;
        let _ = cfg.save(&state.config_path);
    }

    let _ = app.emit("google-login-result", serde_json::json!({ "ok": true }));
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[tauri::command]
fn logout(state: State<AppState>) -> Result<ConfigDto, String> {
    keychain::clear()?;
    let mut cfg = state.config.lock().unwrap();
    cfg.user_email = None;
    cfg.save(&state.config_path).map_err(|e| e.to_string())?;
    Ok(config_dto(&cfg))
}

#[tauri::command]
async fn test_connection(state: State<'_, AppState>) -> Result<String, String> {
    let base_url = state.config.lock().unwrap().base_url.clone();
    if base_url.is_empty() {
        return Err("Configure a URL do PokerSync antes de testar.".into());
    }
    let token = keychain::load()
        .ok_or("Faça login antes de testar a conexão.")?
        .access_token;
    let client = SyncClient::new(base_url, token);
    client.ping().await.map_err(|e| e.to_string())?;
    Ok("Conectado.".to_string())
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mgr = app.autolaunch();
    if enabled {
        mgr.enable().map_err(|e| e.to_string())
    } else {
        mgr.disable().map_err(|e| e.to_string())
    }
}

#[derive(serde::Serialize)]
struct RoomInfo {
    slug: String,
    display_name: String,
    default_folders: Vec<String>,
}

#[tauri::command]
fn list_rooms() -> Vec<RoomInfo> {
    PokerRoom::ALL
        .into_iter()
        .map(|r| RoomInfo {
            slug: r.slug().to_string(),
            display_name: r.display_name().to_string(),
            default_folders: r
                .default_search_paths()
                .into_iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
        })
        .collect()
}

fn roots_for(state: &AppState, room: PokerRoom) -> Vec<PathBuf> {
    let mut roots = room.default_search_paths();
    if let Some(extra) = state.config.lock().unwrap().extra_folders.get(room.slug()) {
        roots.extend(extra.iter().map(PathBuf::from));
    }
    roots
}

#[derive(serde::Serialize)]
struct ScanSummary {
    room: String,
    files_found: usize,
    files_pending: usize,
}

/// Só varre e conta — não sincroniza nada. É o que a UI mostra antes do
/// usuário confirmar "Sincronizar agora".
#[tauri::command]
fn scan_preview(state: State<AppState>, rooms: Vec<String>) -> Result<Vec<ScanSummary>, String> {
    let mut out = Vec::new();
    for slug in rooms {
        let room =
            PokerRoom::from_slug(&slug).ok_or_else(|| format!("Sala desconhecida: {slug}"))?;
        let roots = roots_for(&state, room);
        let found = discover_files(&roots, room);
        let sync_state = SyncState::load(&state.state_dir.join(format!("{}.json", room.slug())));
        let pending = read_pending(&found, &sync_state).len();
        out.push(ScanSummary {
            room: slug,
            files_found: found.len(),
            files_pending: pending,
        });
    }
    Ok(out)
}

#[derive(serde::Serialize)]
struct SyncSummary {
    room: String,
    files_synced: usize,
    total_hands: u32,
    imported: u32,
    duplicates: u32,
    errors: u32,
}

async fn refresh_client(base_url: &str) -> Result<SyncClient, String> {
    let refresh_token = keychain::load()
        .ok_or("Sessão expirada — faça login novamente.")?
        .refresh_token;
    let result = auth::refresh_session(&refresh_token).await?;
    keychain::save(&Tokens {
        access_token: result.access_token.clone(),
        refresh_token: result.refresh_token,
    })?;
    Ok(SyncClient::new(base_url.to_string(), result.access_token))
}

#[tauri::command]
async fn sync_now(
    state: State<'_, AppState>,
    rooms: Vec<String>,
) -> Result<Vec<SyncSummary>, String> {
    let (base_url, token, device) = {
        let cfg = state.config.lock().unwrap();
        if cfg.base_url.is_empty() {
            return Err("Configure a URL do PokerSync antes de sincronizar.".into());
        }
        let token = keychain::load()
            .ok_or("Faça login antes de sincronizar.")?
            .access_token;
        let device = DeviceInfo {
            device_id: cfg.device_id.clone(),
            device_name: cfg.device_name.clone(),
            platform: std::env::consts::OS.to_string(),
            agent_version: env!("CARGO_PKG_VERSION").to_string(),
        };
        (cfg.base_url.clone(), token, device)
    };
    let mut client = SyncClient::new(base_url.clone(), token);
    // Sessão do agente pode ter expirado desde o login — renova uma vez com
    // o refresh_token e recria o client, em vez de forçar login manual de
    // novo a cada sync.
    let mut already_refreshed = false;

    let mut out = Vec::new();
    for slug in rooms {
        let room =
            PokerRoom::from_slug(&slug).ok_or_else(|| format!("Sala desconhecida: {slug}"))?;
        let roots = roots_for(&state, room);
        let found = discover_files(&roots, room);
        let state_path = state.state_dir.join(format!("{}.json", room.slug()));
        let mut sync_state = SyncState::load(&state_path);
        let pending = read_pending(&found, &sync_state);

        if pending.is_empty() {
            out.push(SyncSummary {
                room: slug,
                files_synced: 0,
                total_hands: 0,
                imported: 0,
                duplicates: 0,
                errors: 0,
            });
            continue;
        }

        let files: Vec<SyncFile> = pending
            .iter()
            .map(|p| SyncFile {
                raw_text: p.content.clone(),
                captured_at: None,
            })
            .collect();

        let mut total_hands = 0u32;
        let mut imported = 0u32;
        let mut duplicates = 0u32;
        let mut errors = 0u32;

        for (batch_files, batch_pending) in chunk_files(files, DEFAULT_BATCH_SIZE)
            .into_iter()
            .zip(pending.chunks(DEFAULT_BATCH_SIZE))
        {
            let mut attempt = client.sync_batch(&device, room.slug(), &batch_files).await;
            if let Err(sync_client::SyncError::Rejected { status: 401, .. }) = &attempt {
                if !already_refreshed {
                    already_refreshed = true;
                    client = refresh_client(&base_url).await?;
                    attempt = client.sync_batch(&device, room.slug(), &batch_files).await;
                }
            }
            let result = attempt.map_err(|e| e.to_string())?;
            total_hands += result.total_hands;
            imported += result.imported;
            duplicates += result.duplicates;
            errors += result.errors;
            for p in batch_pending {
                sync_state.mark_synced(p.path.clone(), p.signature);
            }
        }

        sync_state.save(&state_path).map_err(|e| e.to_string())?;
        out.push(SyncSummary {
            room: slug,
            files_synced: pending.len(),
            total_hands,
            imported,
            duplicates,
            errors,
        });
    }
    Ok(out)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            // Argumento passado quando o SO abre o app sozinho no login —
            // usado no setup() abaixo pra abrir minimizado na bandeja em
            // vez de estourar a janela na cara do usuário todo boot.
            Some(vec!["--hidden"]),
        ))
        .setup(|app| {
            let config_dir = app.path().app_config_dir().expect("sem app_config_dir");
            let config_path = config_dir.join("config.json");
            let state_dir = config_dir.join("sync-state");
            let config = AppConfig::load(&config_path);
            app.manage(AppState {
                config_path,
                state_dir,
                config: Mutex::new(config),
                pending_google_state: Mutex::new(None),
            });

            // Login com Google: pokersync-agent://auth?access_token=...
            // volta aqui depois do navegador do sistema completar o OAuth
            // (ver start_google_login e app/agent-login no produto).
            let deep_link_handle = app.handle().clone();
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    if url.scheme() != "pokersync-agent" || url.host_str() != Some("auth") {
                        continue;
                    }
                    let params: std::collections::HashMap<String, String> =
                        url.query_pairs().into_owned().collect();
                    let (Some(access_token), Some(refresh_token)) =
                        (params.get("access_token").cloned(), params.get("refresh_token").cloned())
                    else {
                        continue;
                    };
                    let received_state = params.get("state").cloned().unwrap_or_default();
                    let handle = deep_link_handle.clone();
                    tauri::async_runtime::spawn(async move {
                        complete_google_login(handle, access_token, refresh_token, received_state).await;
                    });
                }
            });

            let show_i = MenuItem::with_id(app, "show", "Mostrar", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_i, &quit_i])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&tray_menu)
                .show_menu_on_left_click(true)
                .tooltip("PokerSync Agent")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            // Iniciado pelo SO no login (--hidden): fica só na bandeja,
            // sem abrir a janela.
            let launched_hidden = std::env::args().any(|a| a == "--hidden");
            if launched_hidden {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            Ok(())
        })
        // Fechar a janela (X) minimiza pra bandeja em vez de encerrar o
        // processo — o agente é feito pra ficar rodando em background.
        // Sair de verdade é só pelo menu da bandeja ("Sair").
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_base_url,
            save_device_name,
            save_extra_folders,
            login,
            start_google_login,
            logout,
            test_connection,
            list_rooms,
            scan_preview,
            sync_now,
            get_autostart,
            set_autostart,
        ])
        .run(tauri::generate_context!())
        .expect("erro ao rodar o app Tauri");
}

#[cfg(test)]
mod tests {
    use super::normalize_base_url;

    #[test]
    fn adds_https_when_scheme_missing() {
        assert_eq!(normalize_base_url("www.pokersync.com.br"), "https://www.pokersync.com.br");
        assert_eq!(normalize_base_url("pokersync.com.br/"), "https://pokersync.com.br");
    }

    #[test]
    fn keeps_explicit_scheme() {
        assert_eq!(normalize_base_url("https://app.pokersync.com/"), "https://app.pokersync.com");
        assert_eq!(normalize_base_url("http://localhost:3000"), "http://localhost:3000");
    }

    #[test]
    fn trims_whitespace_and_empty() {
        assert_eq!(normalize_base_url("  www.pokersync.com.br  "), "https://www.pokersync.com.br");
        assert_eq!(normalize_base_url("   "), "");
    }
}
