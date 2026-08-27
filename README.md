# PokerSync Agent

Agente desktop (Tauri + Rust) que varre o computador do jogador em busca de
hand histories e torneios jogados — PokerStars, GGPoker, PartyPoker e
888poker — e sincroniza com o PokerSync. Implementa a decisão 005 e o item
4 do backlog vivo do produto (ver `POKERSYNC.md` §5/§7 em
[`gsimonetto/pokersync`](https://github.com/gsimonetto/pokersync)).

## Por que assim

- **O agente não faz parsing de mão.** Isso já existe, é validado contra
  hand history real e é bilíngue (EN/PT-BR): `lib/poker/hand-parser.ts`, no
  repo do produto (`gsimonetto/pokersync`). Duplicar essa lógica em Rust
  criaria dois parsers que divergem com o tempo. O agente só encontra
  arquivos, evita reenviar o que não mudou, e manda o texto bruto pro
  backend — quem parseia é o mesmo código que já atende o "colar hand
  history" manual do Revisor.
- **Tauri, não Electron.** Um watcher que roda em background o dia inteiro
  não deveria custar 150MB de RAM parado. WebView nativo do SO + Rust dá um
  binário pequeno e leve pra isso.
- **Repositório próprio.** Mesmo padrão do motor GTO (decisão 009,
  `pokersync-solver`): algo que muda a versão do agente não deve passar
  pelo pipeline de deploy do Next.js, e vice-versa. Extraído do repo do
  produto via `git subtree split` (histórico preservado).

## Estrutura

```
crates/
  scanner/        # descoberta de arquivos + estado de sync (sem parsing de mão)
  sync-client/     # cliente HTTP para /api/agent/{ping,sync}
src-tauri/         # shell Tauri: comandos, config local, login
ui/                # frontend estático (HTML/CSS/JS puro, sem bundler)
```

Do lado do produto, em `gsimonetto/pokersync` (`app/`, `lib/`):

- `app/api/agent/sync/route.ts` — recebe o texto bruto, autentica por
  bearer token (Supabase JWT do usuário).
- `app/api/agent/ping/route.ts` — valida token/conectividade.
- `lib/services/agent-sync-service.ts` — parseia (via `hand-parser.ts`),
  deduplica por `external_hand_id` e grava em `hand_reviews` (`source:
  "agent"`), atualizando `hand_sync_devices`/`hand_sync_batches`.

## Como funciona a varredura

1. Pra cada sala selecionada, `PokerRoom::default_search_paths()` lista
   pastas plausíveis por SO (Documents/AppData/Application Support, por
   variação de skin conhecida). **Best-effort** — cada operadora muda isso
   sem aviso. A UI permite adicionar pastas extras por sala.
2. `discover_files` varre essas pastas recursivamente, filtra por extensão
   e faz uma checagem rápida do início do arquivo (`PokerRoom::sniff`) —
   confirmada contra hand history real só para PokerStars e GGPoker (mesmos
   marcadores do parser). PartyPoker/888poker usam uma heurística mais
   fraca hoje, documentada em `crates/scanner/src/room.rs`.
3. `SyncState` (um JSON por sala, em `app_config_dir()/sync-state/`) guarda
   tamanho+mtime de cada arquivo já sincronizado — só o que mudou desde a
   última vez é relido e reenviado.
4. O texto bruto vai pro backend em lotes de até 50 arquivos
   (`DEFAULT_BATCH_SIZE`); o backend separa múltiplas mãos por arquivo
   (`splitHands`) e deduplica por `external_hand_id` (o handId real, com
   fallback pra hash do texto quando o parser não reconhece o formato).

## Autenticação

V1 pede email/senha diretamente no agente (mesmo GoTrue do produto web) —
a senha nunca é persistida. Os tokens resultantes (access + refresh) ficam
no keychain nativo do SO (`keychain.rs`, via crate `keyring`: Windows
Credential Manager, macOS Keychain, Secret Service no Linux) — nunca em
disco em texto plano. O resto da config (URL, device, pastas) não é
segredo e continua em `config.json`. Próximo passo natural: trocar
email/senha por um fluxo de pareamento por código de uso único, gerado em
`/time` no produto.

## Bandeja do sistema e início automático

Fechar a janela minimiza pra bandeja em vez de encerrar o processo — o
agente é feito pra ficar rodando em background. O menu da bandeja (ícone
perto do relógio) tem "Mostrar" e "Sair" — só "Sair" encerra de verdade. O
toggle "Iniciar automaticamente com o sistema" na tela de Conexão liga o
autostart do SO (`tauri-plugin-autostart`); quando o SO abre o app sozinho
no login, ele já nasce minimizado na bandeja (flag `--hidden`).

## Rodando localmente

```bash
# testes dos crates puros (rápido, sem GUI)
cargo test -p scanner -p sync-client

# dev com hot-reload da UI (precisa de tauri-cli: cargo install tauri-cli --locked)
cargo tauri dev

# build de produção
cargo tauri build
```

Linux precisa de `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`,
`libayatana-appindicator3-dev`, `librsvg2-dev` instalados (ver docs do
Tauri v2 pra Windows/macOS).

## O que falta (próximos passos)

- Fluxo de pareamento por código em vez de email/senha.
- Validar `PokerRoom::default_search_paths` e `sniff` contra instalações
  reais de GGPoker, PartyPoker e 888poker (hoje só PokerStars e GGPoker têm
  parser validado no backend — ver `validateParsedHand` em
  `lib/poker/hand-parser.ts`; PartyPoker/888poker chegam como
  `raw_payload` com `parsed_data` best-effort até o parser ganhar suporte
  a esses formatos).
- Ícone de verdade (hoje é um placeholder azul sólido) e assinatura de
  código por SO (sem isso, Windows/macOS mostram aviso de "app não
  verificado" ao instalar).
- Watcher automático em background (hoje é scan sob demanda, acionado pela
  UI ou manualmente) em vez de só ficar disponível na bandeja.
