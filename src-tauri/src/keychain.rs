//! Tokens de sessão (access + refresh) guardados no keychain nativo do SO
//! — Windows Credential Manager, macOS Keychain, Secret Service no Linux —
//! em vez de arquivo texto plano. O resto da config (URL, device, pastas)
//! não é segredo e continua em `config.rs`.
//!
//! O Windows Credential Manager recusa uma "senha" com mais de 2560
//! caracteres (erro "Attribute 'password encoded as UTF-16' is longer
//! than platform limit of 2560 chars") — e o JSON com os dois tokens
//! juntos passa disso com folga quando o access_token é um JWT grande.
//! Por isso guardamos em pedaços menores, um por entrada do keychain, em
//! vez de um bloco só.

use keyring::Entry;
use serde::{Deserialize, Serialize};

const SERVICE: &str = "com.pokersync.radar";
/// Nome usado antes do app virar "Radar PokerSync" — mantido só de leitura
/// pra quem já tinha feito login não ser deslogado na atualização.
const SERVICE_LEGACY: &str = "com.pokersync.agent";
const ACCOUNT_LEGACY: &str = "session";
const ACCOUNT_COUNT: &str = "session-chunk-count";

/// Bem abaixo do limite de 2560 caracteres do Windows — sobra margem pra
/// backends de outros SOs, que costumam ser mais folgados.
const CHUNK_SIZE: usize = 2000;
/// Trava de segurança contra um número de pedaços absurdo (ex.: entrada
/// de contagem corrompida) — nenhum token real chega perto disso.
const MAX_CHUNKS: usize = 50;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
}

fn entry(account: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, account).map_err(|e| format!("Keychain indisponível: {e}"))
}

fn legacy_entry() -> Result<Entry, String> {
    Entry::new(SERVICE_LEGACY, ACCOUNT_LEGACY).map_err(|e| format!("Keychain indisponível: {e}"))
}

fn chunk_account(index: usize) -> String {
    format!("session-chunk-{index}")
}

fn read_chunk_count() -> Option<usize> {
    let raw = entry(ACCOUNT_COUNT).ok()?.get_password().ok()?;
    raw.parse::<usize>().ok().filter(|n| *n <= MAX_CHUNKS)
}

fn load_chunked() -> Option<Tokens> {
    let count = read_chunk_count()?;
    let mut raw = String::new();
    for i in 0..count {
        raw.push_str(&entry(&chunk_account(i)).ok()?.get_password().ok()?);
    }
    serde_json::from_str(&raw).ok()
}

/// Sessões salvas antes dessa correção (formato antigo) ou antes do app
/// virar "Radar PokerSync" (nome/serviço antigo) — sem isso, todo mundo
/// que já tinha logado seria deslogado na próxima atualização do agente.
fn load_legacy() -> Option<Tokens> {
    let same_service = entry(ACCOUNT_LEGACY).ok().and_then(|e| e.get_password().ok());
    let old_service = legacy_entry().ok().and_then(|e| e.get_password().ok());
    let raw = same_service.or(old_service)?;
    serde_json::from_str(&raw).ok()
}

/// `None` tanto quando não há sessão salva quanto quando o backend do
/// keychain falha (ex.: ambiente sem Secret Service no Linux) — nesse caso
/// o app trata como "não logado" e pede login de novo, em vez de travar.
pub fn load() -> Option<Tokens> {
    load_chunked().or_else(load_legacy)
}

pub fn save(tokens: &Tokens) -> Result<(), String> {
    let raw = serde_json::to_string(tokens).map_err(|e| e.to_string())?;
    let chars: Vec<char> = raw.chars().collect();
    let chunks: Vec<String> = chars
        .chunks(CHUNK_SIZE)
        .map(|c| c.iter().collect())
        .collect();
    // Não deveria acontecer com tokens reais, mas mais vale falhar com uma
    // mensagem clara do que gravar pela metade.
    if chunks.len() > MAX_CHUNKS {
        return Err(format!(
            "Sessão grande demais para o keychain ({} pedaços).",
            chunks.len()
        ));
    }

    let previous_count = read_chunk_count().unwrap_or(0);

    for (i, chunk) in chunks.iter().enumerate() {
        entry(&chunk_account(i))?
            .set_password(chunk)
            .map_err(|e| e.to_string())?;
    }
    entry(ACCOUNT_COUNT)?
        .set_password(&chunks.len().to_string())
        .map_err(|e| e.to_string())?;

    // Sessão nova tem menos pedaços que a anterior (ex.: refresh_token
    // encolheu) — limpa o que sobrou pra não ficar lixo nem confundir uma
    // leitura futura com contagem errada.
    for i in chunks.len()..previous_count {
        let _ = entry(&chunk_account(i)).and_then(|e| match e.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err.to_string()),
        });
    }

    // Migração: uma vez salvo no formato novo, as entradas antigas (formato
    // de senha única e/ou nome de serviço de antes do rename) não servem
    // mais pra nada — remove pra não deixar token velho parado no cofre do SO.
    let _ = entry(ACCOUNT_LEGACY).and_then(|e| match e.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(err.to_string()),
    });
    let _ = legacy_entry().and_then(|e| match e.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(err.to_string()),
    });

    Ok(())
}

pub fn clear() -> Result<(), String> {
    let count = read_chunk_count().unwrap_or(0);
    for i in 0..count {
        match entry(&chunk_account(i))?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    match entry(ACCOUNT_COUNT)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(e) => return Err(e.to_string()),
    }
    match entry(ACCOUNT_LEGACY)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(e) => return Err(e.to_string()),
    }
    match legacy_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
