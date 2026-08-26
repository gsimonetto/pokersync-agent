# PokerSync Agent

Agente desktop (Tauri + Rust) que varre o computador do jogador em busca de
hand histories e torneios jogados — PokerStars, GGPoker, PartyPoker e
888poker — e sincroniza com o PokerSync. Implementa a decisão 005
(`POKERSYNC.md` §5) e o item 4 do backlog (§7).

## Por que assim

- **O agente não faz parsing de mão.** Isso já existe, é validado contra
  hand history real e é bilíngue (EN/PT-BR): `lib/poker/hand-parser.ts`, no
  repo do produto. Duplicar essa lógica em Rust criaria dois parsers que
  divergem com o tempo. O agente só encontra arquivos, evita reenviar o que
  não mudou, e manda o texto bruto pro backend — quem parseia é o mesmo
  código que já atende o "colar hand history" manual do Revisor.
- **Tauri, não Electron.** Um watcher que roda em background o dia inteiro
  não deveria custar 150MB de RAM parado. WebView nativo do SO + Rust dá um
  binário pequeno e leve pra isso.
- **Fica num monorepo por ora.** O ideal (mesmo padrão do motor GTO,
  decisão 009) é um repositório próprio (`pokersync-agent`), com deploy e
  versionamento independentes — algo que muda a versão do agente não deve
  passar pelo pipeline do Next.js, e vice-versa. Essa sessão não conseguiu
  criar o repositório (a integração de GitHub usada aqui não tem permissão
  de `create_repository`); o código está isolado em `agent-desktop/` como
  primeiro passo, pronto pra ser extraído com `git subtree split` quando o
  repo existir.

## Estrutura

```
agent-desktop/
  crates/
    scanner/       # descoberta de arquivos + estado de sync (sem parsing de mão)
    sync-client/    # cliente HTTP para /api/agent/{ping,sync}
  src-tauri/        # shell Tauri: comandos, config local, login
  ui/               # frontend estático (HTML/CSS/JS puro, sem bundler)
```

Do lado do produto (`app/`, `lib/`, neste mesmo repo por enquanto):

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
a senha nunca é persistida, só os tokens resultantes, salvos em texto
plano em `app_config_dir()/config.json`. Aceitável pra uma sessão local de
agente; **não é produção-grade**. Próximo passo natural: um fluxo de
pareamento por código de uso único gerado em `/time` no produto, e migrar
os tokens pro keychain do SO (`keyring` crate) em vez de arquivo plano.

## Rodando localmente

```bash
# testes dos crates puros (rápido, sem GUI)
cargo test -p scanner -p sync-client

# dev com hot-reload da UI (precisa de tauri-cli: cargo install tauri-cli --locked)
cd agent-desktop && cargo tauri dev

# build de produção
cd agent-desktop && cargo tauri build
```

Linux precisa de `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`,
`libayatana-appindicator3-dev`, `librsvg2-dev` instalados (ver docs do
Tauri v2 pra Windows/macOS).

## O que falta (próximos passos, não desta sessão)

- Repositório próprio (`pokersync-agent`) e CI de release por SO.
- Fluxo de pareamento por código em vez de email/senha; tokens no keychain
  do SO.
- Validar `PokerRoom::default_search_paths` e `sniff` contra instalações
  reais de GGPoker, PartyPoker e 888poker (hoje só PokerStars e GGPoker têm
  parser validado no backend — ver `validateParsedHand` em
  `lib/poker/hand-parser.ts`; PartyPoker/888poker chegam como
  `raw_payload` com `parsed_data` best-effort até o parser ganhar suporte
  a esses formatos).
- Ícone/tray real, autostart no login do SO, watcher em background (hoje é
  scan sob demanda, acionado pela UI).
