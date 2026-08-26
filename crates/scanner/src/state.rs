use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Assinatura de um arquivo já sincronizado: tamanho + mtime (epoch
/// segundos). Suficiente pra detectar "arquivo cresceu / mudou" sem
/// precisar reler+hashear tudo a cada scan — hand history só cresce
/// (append-only pelos clientes de poker), então isso raramente dá falso
/// negativo na prática.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSignature {
    pub size: u64,
    pub modified_unix: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SyncState {
    synced_files: HashMap<PathBuf, FileSignature>,
}

impl SyncState {
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self).unwrap_or_default();
        fs::write(path, json)
    }

    /// Um arquivo precisa ser (re)sincronizado se nunca foi visto antes, ou
    /// se mudou de tamanho/mtime desde o último sync (ex.: novas mãos
    /// anexadas no fim do arquivo do dia).
    pub fn needs_sync(&self, path: &Path, sig: FileSignature) -> bool {
        self.synced_files.get(path) != Some(&sig)
    }

    pub fn mark_synced(&mut self, path: PathBuf, sig: FileSignature) {
        self.synced_files.insert(path, sig);
    }
}

pub fn signature_of(path: &Path) -> std::io::Result<FileSignature> {
    let meta = fs::metadata(path)?;
    let modified_unix = meta
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    Ok(FileSignature {
        size: meta.len(),
        modified_unix,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        let file_path = dir.path().join("hand.txt");
        std::fs::write(&file_path, "hello").unwrap();
        let sig = signature_of(&file_path).unwrap();

        let mut state = SyncState::load(&state_path);
        assert!(state.needs_sync(&file_path, sig));
        state.mark_synced(file_path.clone(), sig);
        state.save(&state_path).unwrap();

        let reloaded = SyncState::load(&state_path);
        assert!(!reloaded.needs_sync(&file_path, sig));
    }

    #[test]
    fn detects_growth() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("hand.txt");
        std::fs::write(&file_path, "hello").unwrap();
        let sig1 = signature_of(&file_path).unwrap();

        let mut state = SyncState::default();
        state.mark_synced(file_path.clone(), sig1);

        std::fs::write(&file_path, "hello world, more hands appended").unwrap();
        let sig2 = signature_of(&file_path).unwrap();
        assert!(state.needs_sync(&file_path, sig2));
    }
}
