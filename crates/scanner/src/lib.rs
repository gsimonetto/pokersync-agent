//! Varredura de hand history no disco do usuário. Não faz parsing de mão —
//! isso já existe (e é validado contra formatos reais) no backend, em
//! lib/poker/hand-parser.ts. Este crate só encontra arquivos plausíveis,
//! evita reenviar o que não mudou, e devolve texto bruto pra sincronizar.

pub mod room;
pub mod state;

pub use room::PokerRoom;
pub use state::{signature_of, FileSignature, SyncState};

use std::io::Read;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const MAX_FILE_BYTES: u64 = 20 * 1024 * 1024; // hand history de anos ainda cabe tranquilo
const SNIFF_BYTES: usize = 4096;

#[derive(Debug, Clone)]
pub struct DiscoveredFile {
    pub path: PathBuf,
    pub room: PokerRoom,
    pub signature: FileSignature,
}

#[derive(Debug, Clone)]
pub struct PendingFile {
    pub path: PathBuf,
    pub room: PokerRoom,
    pub content: String,
    pub signature: FileSignature,
}

fn has_text_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("txt") || e.eq_ignore_ascii_case("log"))
        .unwrap_or(false)
}

fn sniff_file(path: &Path, room: PokerRoom) -> bool {
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut buf = vec![0u8; SNIFF_BYTES];
    let Ok(n) = f.read(&mut buf) else {
        return false;
    };
    buf.truncate(n);
    room.sniff(&String::from_utf8_lossy(&buf))
}

/// Varre `roots` recursivamente procurando hand history da sala indicada.
/// Só lê os primeiros bytes de cada arquivo candidato (sniff) — o conteúdo
/// inteiro só é lido depois, em `read_pending`, e só pros arquivos que
/// realmente precisam sincronizar.
pub fn discover_files(roots: &[PathBuf], room: PokerRoom) -> Vec<DiscoveredFile> {
    let mut found = Vec::new();
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        for entry in WalkDir::new(root)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let path = entry.path();
            if !has_text_extension(path) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if meta.len() == 0 || meta.len() > MAX_FILE_BYTES {
                continue;
            }
            if !sniff_file(path, room) {
                continue;
            }
            let Ok(signature) = signature_of(path) else {
                continue;
            };
            found.push(DiscoveredFile {
                path: path.to_path_buf(),
                room,
                signature,
            });
        }
    }
    found
}

/// Filtra `files` pelos que mudaram desde o último sync (via `state`) e lê
/// o conteúdo inteiro só desses.
pub fn read_pending(files: &[DiscoveredFile], state: &SyncState) -> Vec<PendingFile> {
    files
        .iter()
        .filter(|f| state.needs_sync(&f.path, f.signature))
        .filter_map(|f| {
            std::fs::read_to_string(&f.path)
                .ok()
                .map(|content| PendingFile {
                    path: f.path.clone(),
                    room: f.room,
                    content,
                    signature: f.signature,
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn discovers_pokerstars_hand_history_and_ignores_junk() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "HH20260101 Table.txt",
            "PokerStars Hand #1234: Tournament #1, $10+$1 USD Hold'em No Limit\n...",
        );
        write(
            dir.path(),
            "readme.md",
            "PokerStars Hand #1234: not a hand history file, wrong extension",
        );
        write(
            dir.path(),
            "notes.txt",
            "just some notes, not a hand history",
        );
        write(dir.path(), "empty.txt", "");

        let found = discover_files(&[dir.path().to_path_buf()], PokerRoom::PokerStars);
        assert_eq!(found.len(), 1);
        assert!(found[0].path.ends_with("HH20260101 Table.txt"));
    }

    #[test]
    fn recurses_into_subdirectories() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("2026-08");
        std::fs::create_dir_all(&sub).unwrap();
        write(
            &sub,
            "HH.txt",
            "PokerStars Hand #999: Hold'em No Limit\n...",
        );

        let found = discover_files(&[dir.path().to_path_buf()], PokerRoom::PokerStars);
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn missing_root_is_skipped_not_error() {
        let found = discover_files(
            &[PathBuf::from("/this/path/does/not/exist")],
            PokerRoom::PokerStars,
        );
        assert!(found.is_empty());
    }

    #[test]
    fn read_pending_skips_unchanged_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = write(
            dir.path(),
            "HH.txt",
            "PokerStars Hand #1: Hold'em No Limit\n...",
        );
        let found = discover_files(&[dir.path().to_path_buf()], PokerRoom::PokerStars);
        assert_eq!(found.len(), 1);

        let mut state = SyncState::default();
        let pending_before = read_pending(&found, &state);
        assert_eq!(pending_before.len(), 1);

        state.mark_synced(path.clone(), found[0].signature);
        let pending_after = read_pending(&found, &state);
        assert!(pending_after.is_empty());
    }
}
