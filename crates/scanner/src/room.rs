use std::path::PathBuf;

/// Dois tipos de arquivo que o agente varre — hand history (mãos jogadas)
/// e resumo de torneio (buy-in/colocação/premiação, sem as mãos). São
/// arquivos diferentes, em pastas diferentes, e alimentam endpoints
/// diferentes no backend (ver `kind_to_endpoint` no crate `sync-client` e
/// os dois botões "Importar mãos"/"Importar torneios" na UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    HandHistory,
    TournamentSummary,
}

impl FileKind {
    pub fn slug(self) -> &'static str {
        match self {
            FileKind::HandHistory => "hands",
            FileKind::TournamentSummary => "tournaments",
        }
    }

    pub fn from_slug(slug: &str) -> Option<FileKind> {
        match slug {
            "hands" => Some(FileKind::HandHistory),
            "tournaments" => Some(FileKind::TournamentSummary),
            _ => None,
        }
    }
}

/// Salas de poker suportadas no MVP do agente. `slug()` é o valor enviado
/// pro backend (coluna `poker_room` de `hand_reviews`/`hand_sync_batches`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PokerRoom {
    PokerStars,
    GgPoker,
    PartyPoker,
    Poker888,
    Acr,
}

impl PokerRoom {
    pub const ALL: [PokerRoom; 5] = [
        PokerRoom::PokerStars,
        PokerRoom::GgPoker,
        PokerRoom::PartyPoker,
        PokerRoom::Poker888,
        PokerRoom::Acr,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            PokerRoom::PokerStars => "pokerstars",
            PokerRoom::GgPoker => "ggpoker",
            PokerRoom::PartyPoker => "partypoker",
            PokerRoom::Poker888 => "888poker",
            PokerRoom::Acr => "acr",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            PokerRoom::PokerStars => "PokerStars",
            PokerRoom::GgPoker => "GGPoker",
            PokerRoom::PartyPoker => "PartyPoker",
            PokerRoom::Poker888 => "888poker",
            PokerRoom::Acr => "ACR",
        }
    }

    pub fn from_slug(slug: &str) -> Option<PokerRoom> {
        PokerRoom::ALL.into_iter().find(|r| r.slug() == slug)
    }

    /// Nomes de pasta conhecidos do cliente dessa sala, por variação de
    /// skin/país — hand history sempre fica numa subpasta "HandHistory"
    /// dentro dela. Best-effort: cada operadora muda isso sem aviso, então
    /// isto é só o ponto de partida da varredura — o usuário pode (e deve
    /// poder) apontar pastas adicionais na UI do agente.
    fn client_folder_names(self) -> &'static [&'static str] {
        match self {
            PokerRoom::PokerStars => &[
                "PokerStars",
                "PokerStarsIT",
                "PokerStarsFR",
                "PokerStarsES",
                "PokerStarsPT",
                "PokerStars.ES",
            ],
            PokerRoom::GgPoker => &["GGPoker", "GGNetwork", "Natural8"],
            PokerRoom::PartyPoker => &["PartyGaming/PartyPoker", "partypoker"],
            PokerRoom::Poker888 => &["888poker", "888 Poker"],
            // ACR roda no cliente da Winning Poker Network — pasta de
            // instalação varia por skin (ACR, Black Chip Poker, True
            // Poker), mas hand history geralmente vai em "HH" em vez de
            // "HandHistory" (ver default_search_paths).
            PokerRoom::Acr => &["ACR Poker", "AmericasCardroom", "Americas Cardroom"],
        }
    }

    /// Nome(s) da subpasta de hand history dentro da pasta do cliente.
    /// A maioria usa "HandHistory"; a Winning Poker Network (ACR) usa "HH".
    fn history_subfolder_names(self) -> &'static [&'static str] {
        match self {
            PokerRoom::Acr => &["HH", "HandHistory"],
            _ => &["HandHistory"],
        }
    }

    /// Nome(s) da subpasta de resumo de torneio (Tournament Summary)
    /// dentro da pasta do cliente — arquivo separado da hand history, com
    /// buy-in/colocação/premiação em vez das mãos jogadas. Só o nome
    /// PokerStars ("TournamentSummary") é confirmado; as demais salas
    /// caem no mesmo nome por falta de amostra real — mesmo status
    /// "best-effort" que `client_folder_names`/`sniff` já documentam pra
    /// PartyPoker/888poker/ACR.
    fn tournament_summary_subfolder_names(self) -> &'static [&'static str] {
        match self {
            PokerRoom::PokerStars => &["TournamentSummary"],
            PokerRoom::Acr => &["TS", "TournamentSummary"],
            _ => &["TournamentSummary"],
        }
    }

    fn subfolder_names(self, kind: FileKind) -> &'static [&'static str] {
        match kind {
            FileKind::HandHistory => self.history_subfolder_names(),
            FileKind::TournamentSummary => self.tournament_summary_subfolder_names(),
        }
    }

    /// Pastas onde o cliente dessa sala plausivelmente grava hand history
    /// ou resumo de torneio (`kind`), para o sistema operacional atual.
    /// Caminhos que não existem no disco são descartados por quem chama
    /// (ver `discover_files`), então listar candidatos "a mais" aqui é
    /// seguro.
    pub fn default_search_paths(self, kind: FileKind) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        let home = dirs::home_dir();
        let documents = dirs::document_dir();
        let config = dirs::config_dir(); // %APPDATA% no Windows, ~/.config no Linux
        let data_local = dirs::data_local_dir(); // %LOCALAPPDATA% no Windows

        for folder in self.client_folder_names() {
            for sub in self.subfolder_names(kind) {
                if let Some(doc) = &documents {
                    roots.push(doc.join(folder).join(sub));
                }
                if let Some(cfg) = &config {
                    roots.push(cfg.join(folder).join(sub));
                }
                if let Some(local) = &data_local {
                    roots.push(local.join(folder).join(sub));
                }
                // macOS: clientes de poker costumam gravar em Application Support.
                if let Some(h) = &home {
                    roots.push(h.join("Library/Application Support").join(folder).join(sub));
                }
            }
        }
        roots
    }

    /// Heurística barata pra reconhecer se um texto é hand history dessa
    /// sala, olhando só o início do arquivo (evita ler/parsear arquivos
    /// grandes por completo só pra descartá-los). PokerStars e GGPoker são
    /// confirmados contra hand history real (mesmos marcadores do parser em
    /// lib/poker/hand-parser.ts); PartyPoker, 888poker e ACR são best-effort
    /// — não temos amostra real ainda, então a varredura confia principalmente
    /// na pasta de origem (client_folder_names) e só usa isto como reforço.
    pub fn sniff(self, head: &str) -> bool {
        match self {
            PokerRoom::PokerStars => {
                head.contains("PokerStars Hand #") || head.contains("Mão PokerStars #")
            }
            PokerRoom::GgPoker => head.contains("GGPoker Hand") || head.contains("Poker Hand #"),
            PokerRoom::PartyPoker => {
                head.to_lowercase().contains("partypoker") || head.contains("Game #")
            }
            PokerRoom::Poker888 => head.contains("888poker") || head.contains("Game #"),
            PokerRoom::Acr => {
                let lower = head.to_lowercase();
                lower.contains("winning poker network")
                    || lower.contains("americas cardroom")
                    || head.contains("Stage #")
            }
        }
    }

    /// Mesma ideia de `sniff`, pro resumo de torneio em vez da mão. Só
    /// PokerStars é confirmado (formato documentado, "PokerStars
    /// Tournament #NNN" / "Torneio PokerStars #NNN" no cabeçalho); as
    /// demais salas são best-effort, sem amostra real — mesmo status do
    /// resto do arquivo (ver `sniff`).
    pub fn sniff_tournament_summary(self, head: &str) -> bool {
        match self {
            PokerRoom::PokerStars => {
                head.contains("PokerStars Tournament #") || head.contains("Torneio PokerStars #")
            }
            PokerRoom::GgPoker => head.contains("GGPoker Tournament") || head.contains("Tournament #"),
            PokerRoom::PartyPoker => {
                head.to_lowercase().contains("partypoker") && head.to_lowercase().contains("tournament")
            }
            PokerRoom::Poker888 => head.contains("888poker") && head.to_lowercase().contains("tournament"),
            PokerRoom::Acr => {
                let lower = head.to_lowercase();
                (lower.contains("winning poker network") || lower.contains("americas cardroom"))
                    && lower.contains("tournament")
            }
        }
    }

    pub fn sniff_kind(self, kind: FileKind, head: &str) -> bool {
        match kind {
            FileKind::HandHistory => self.sniff(head),
            FileKind::TournamentSummary => self.sniff_tournament_summary(head),
        }
    }
}
