use std::path::PathBuf;

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

    /// Pastas onde o cliente dessa sala plausivelmente grava hand history,
    /// para o sistema operacional atual. Caminhos que não existem no disco
    /// são descartados por quem chama (ver `discover_files`), então listar
    /// candidatos "a mais" aqui é seguro.
    pub fn default_search_paths(self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        let home = dirs::home_dir();
        let documents = dirs::document_dir();
        let config = dirs::config_dir(); // %APPDATA% no Windows, ~/.config no Linux
        let data_local = dirs::data_local_dir(); // %LOCALAPPDATA% no Windows

        for folder in self.client_folder_names() {
            for sub in self.history_subfolder_names() {
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
}
