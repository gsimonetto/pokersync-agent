# Auto-update — configurar os secrets do GitHub

O auto-update já está implementado no código (plugin `tauri-plugin-updater`,
chave pública em `src-tauri/tauri.conf.json`, checagem em `ui/app.js`). Falta
só um passo manual, único, pra ativar de verdade: cadastrar a chave privada
de assinatura como secret do repositório.

## Por quê

Cada build assinado com a chave privada; o app instalado confere a
assinatura com a chave pública (já está no `tauri.conf.json`) antes de
instalar qualquer atualização. Sem a chave privada configurada no CI, o
workflow de release ainda gera os instaladores normalmente, mas não gera o
`latest.json`/`.sig` — ou seja, builda mas o auto-update não funciona.

## Passo a passo

1. Vá em **Settings → Secrets and variables → Actions** neste repositório
   (`gsimonetto/pokersync-agent`).
2. Clique em **New repository secret** e crie os dois:
   - `TAURI_SIGNING_PRIVATE_KEY` — conteúdo do arquivo `.key` gerado por
     `cargo tauri signer generate` (uma string longa em base64).
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — a senha escolhida ao gerar a
     chave.

Se você não guardou esses dois valores (foram mostrados uma única vez na
sessão que implementou isso), gere um par novo:

```bash
cargo install tauri-cli --version "^2" --locked   # se ainda não tiver
cargo tauri signer generate -w ./pokersync-agent.key
```

Isso imprime o caminho da chave privada e a chave pública. Se gerar uma
chave **nova**, também precisa atualizar `pubkey` em
`src-tauri/tauri.conf.json` (`plugins.updater.pubkey`) com o conteúdo do
arquivo `.key.pub` gerado — senão os apps já instalados (assinados com a
chave antiga) não vão confiar nos builds novos.

## Depois de configurar

Qualquer push de tag `vX.Y.Z` (`git tag v0.2.0 && git push --tags`) já
builda, assina e publica a release de verdade (não mais rascunho) — os
agentes já instalados verificam a versão mais nova a cada login e mostram
um banner "Nova versão disponível" quando encontram uma.
