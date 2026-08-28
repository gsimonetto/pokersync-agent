# PokerSync Agent

Agente desktop (Tauri + Rust) que varre o computador do jogador em busca de
hand histories e torneios jogados — PokerStars, GGPoker, PartyPoker, 888poker
e ACR — e sincroniza com o PokerSync. Implementa a decisão 005 e o item
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
   marcadores do parser). PartyPoker/888poker/ACR usam uma heurística mais
   fraca hoje, documentada em `crates/scanner/src/room.rs`.
3. `SyncState` (um JSON por sala, em `app_config_dir()/sync-state/`) guarda
   tamanho+mtime de cada arquivo já sincronizado — só o que mudou desde a
   última vez é relido e reenviado.
4. O texto bruto vai pro backend em lotes de até 50 arquivos
   (`DEFAULT_BATCH_SIZE`); o backend separa múltiplas mãos por arquivo
   (`splitHands`) e deduplica por `external_hand_id` (o handId real, com
   fallback pra hash do texto quando o parser não reconhece o formato).

## Autenticação

Dois caminhos, ambos contra o mesmo GoTrue do produto web:

- **Email/senha**: direto na janela do agente. A senha nunca é persistida.
- **Google**: o OAuth do Google não roda dentro da webview embutida do
  Tauri (Google bloqueia login em iframes/webviews embutidas por
  política de segurança) — o botão "Continuar com Google" abre o
  **navegador do sistema** numa página dedicada do produto
  (`gsimonetto/pokersync`, `app/agent-login/`), que faz o OAuth normal e
  devolve os tokens pro agente via deep link (`pokersync-agent://auth`,
  registrado pelo instalador — `tauri-plugin-deep-link`). Isso resolve o
  caso relatado de "logou pelo Google, a senha não pega aqui" — quem
  criou a conta assim nunca teve senha no Supabase pra começar.
  Um nonce (`state`) gerado antes de abrir o navegador e conferido na
  volta impede que um deep link de outra origem seja aceito como se
  fosse resposta desse login.

Em ambos os casos, os tokens resultantes (access + refresh) ficam no
keychain nativo do SO (`keychain.rs`, via crate `keyring`: Windows
Credential Manager, macOS Keychain, Secret Service no Linux) — nunca em
disco em texto plano. O resto da config (URL, device, pastas) não é
segredo e continua em `config.json`. Próximo passo natural: um fluxo de
pareamento por código de uso único (sem precisar de email/senha nem
depender do navegador do sistema), gerado em `/time` no produto.

## URL do PokerSync

O domínio de produção (`DEFAULT_BASE_URL` em `config.rs`) vem embutido no
binário — o jogador nunca vê nem precisa configurar isso. O campo "URL do
PokerSync" só existe dentro de "Configurações avançadas" (escondido por
padrão), pra depuração (ambiente de teste, self-host).

## Bandeja do sistema e início automático

Fechar a janela minimiza pra bandeja em vez de encerrar o processo — o
agente é feito pra ficar rodando em background. O menu da bandeja (ícone
perto do relógio) tem "Mostrar" e "Sair" — só "Sair" encerra de verdade. O
toggle "Iniciar automaticamente com o sistema" em Configurações avançadas
liga o autostart do SO (`tauri-plugin-autostart`); quando o SO abre o app
sozinho no login, ele já nasce minimizado na bandeja (flag `--hidden`).

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

## Identidade visual

`ui/` usa os mesmos tokens do produto web (`app/globals.css` em
`gsimonetto/pokersync`): fundo `--void` (#000), cards `--surface`/
`--elevated`, tipografia Space Grotesk, e a mesma paleta de acentos por
módulo (`--positive`/`--negative`/`--training`/`--evolution`/`--review`).
O logo (`pokersync-logo.svg`) é o arquivo real do produto, copiado pra cá.

**Badges das salas**: não são os logotipos oficiais de PokerStars/GGPoker/
PartyPoker/888poker/ACR — são iniciais num badge colorido, usando só as cores
do próprio design system do PokerSync (`ROOM_STYLE` em `ui/app.js`), não
as cores de marca de cada operadora. Decisão deliberada: fabricar de
memória um logotipo de terceiro é arriscado (fica errado, ou levanta
questão de uso de marca sem aprovação) — os badges atuais são um
placeholder honesto até alguém do time aprovar os assets oficiais de
cada sala pra substituir.

## O que falta (próximos passos)

- Trocar os badges de iniciais pelos logotipos oficiais de cada sala
  (precisa dos assets aprovados — ver "Identidade visual" acima).
- Fluxo de pareamento por código em vez de email/senha/Google.
- Validar `PokerRoom::default_search_paths` e `sniff` contra instalações
  reais de GGPoker, PartyPoker, 888poker e ACR (hoje só PokerStars e GGPoker
  têm parser validado no backend — ver `validateParsedHand` em
  `lib/poker/hand-parser.ts`; PartyPoker/888poker/ACR chegam como
  `raw_payload` com `parsed_data` best-effort até o parser ganhar suporte
  a esses formatos — `hand-parser.ts` hoje nem reconhece o texto de hand
  history dessas três salas, é o maior gap real da varredura hoje).
- Ícone de verdade (hoje é um placeholder azul sólido) e assinatura de
  código por SO (sem isso, Windows/macOS mostram aviso de "app não
  verificado" ao instalar).
- Watcher automático em background (hoje é scan sob demanda, acionado pela
  UI ou manualmente) em vez de só ficar disponível na bandeja.
