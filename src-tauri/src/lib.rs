mod auth;
mod config;

use config::AppConfig;
use scanner::{discover_files, read_pending, PokerRoom, SyncState};
use std::path::PathBuf;
use std::sync::Mutex;
use sync_client::{chunk_files, DeviceInfo, SyncClient, SyncFile, DEFAULT_BATCH_SIZE};
use tauri::{Manager, State};

struct AppState {
    config_path: PathBuf,
    state_dir: PathBuf,
    config: Mutex<AppConfig>,
}

#[derive(serde::Serialize)]
struct ConfigDto {
    base_url: String,
    logged_in: bool,
    user_email: Option<String>,
    device_name: String,
    extra_folders: std::collections::HashMap<String, Vec<String>>,
}

impl From<&AppConfig> for ConfigDto {
    fn from(c: &AppConfig) -> Self {
        Self {
            base_url: c.base_url.clone(),
            logged_in: c.is_logged_in(),
            user_email: c.user_email.clone(),
            device_name: c.device_name.clone(),
            extra_folders: c.extra_folders.clone(),
        }
    }
}

#[tauri::command]
fn get_config(state: State<AppState>) -> ConfigDto {
    ConfigDto::from(&*state.config.lock().unwrap())
}

#[tauri::command]
fn save_base_url(state: State<AppState>, base_url: String) -> Result<(), String> {
    let mut cfg = state.config.lock().unwrap();
    cfg.base_url = base_url.trim().trim_end_matches('/').to_string();
    cfg.save(&state.config_path).map_err(|e| e.to_string())
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
    let mut cfg = state.config.lock().unwrap();
    cfg.access_token = Some(result.access_token);
    cfg.refresh_token = Some(result.refresh_token);
    cfg.user_email = result.email;
    cfg.save(&state.config_path).map_err(|e| e.to_string())?;
    Ok(ConfigDto::from(&*cfg))
}

#[tauri::command]
fn logout(state: State<AppState>) -> Result<ConfigDto, String> {
    let mut cfg = state.config.lock().unwrap();
    cfg.access_token = None;
    cfg.refresh_token = None;
    cfg.user_email = None;
    cfg.save(&state.config_path).map_err(|e| e.to_string())?;
    Ok(ConfigDto::from(&*cfg))
}

#[tauri::command]
async fn test_connection(state: State<'_, AppState>) -> Result<String, String> {
    let (base_url, token) = {
        let cfg = state.config.lock().unwrap();
        (cfg.base_url.clone(), cfg.access_token.clone())
    };
    if base_url.is_empty() {
        return Err("Configure a URL do PokerSync antes de testar.".into());
    }
    let token = token.ok_or("Faça login antes de testar a conexão.")?;
    let client = SyncClient::new(base_url, token);
    client.ping().await.map_err(|e| e.to_string())?;
    Ok("Conectado.".to_string())
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

async fn refresh_client(state: &AppState, base_url: &str) -> Result<SyncClient, String> {
    let refresh_token = state
        .config
        .lock()
        .unwrap()
        .refresh_token
        .clone()
        .ok_or("Sessão expirada — faça login novamente.")?;
    let result = auth::refresh_session(&refresh_token).await?;
    let mut cfg = state.config.lock().unwrap();
    cfg.access_token = Some(result.access_token.clone());
    cfg.refresh_token = Some(result.refresh_token);
    cfg.save(&state.config_path).map_err(|e| e.to_string())?;
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
        let token = cfg
            .access_token
            .clone()
            .ok_or("Faça login antes de sincronizar.")?;
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
                    client = refresh_client(&state, &base_url).await?;
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
        .setup(|app| {
            let config_dir = app.path().app_config_dir().expect("sem app_config_dir");
            let config_path = config_dir.join("config.json");
            let state_dir = config_dir.join("sync-state");
            let config = AppConfig::load(&config_path);
            app.manage(AppState {
                config_path,
                state_dir,
                config: Mutex::new(config),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_base_url,
            save_device_name,
            save_extra_folders,
            login,
            logout,
            test_connection,
            list_rooms,
            scan_preview,
            sync_now,
        ])
        .run(tauri::generate_context!())
        .expect("erro ao rodar o app Tauri");
}
