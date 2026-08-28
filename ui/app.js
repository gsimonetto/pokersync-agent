const invoke = window.__TAURI__.core.invoke;
const openFolderDialog = window.__TAURI__.dialog.open;
const listen = window.__TAURI__.event.listen;

const el = (id) => document.getElementById(id);

// Cores de acento — do próprio design system do PokerSync (não são as
// cores de marca de cada sala; ver decisão em ui/README caso exista).
// Cada sala recebe um acento fixo só pra diferenciar visualmente os
// cards, com um badge de iniciais no lugar de logotipos de terceiros.
const ROOM_STYLE = {
  pokerstars: { initials: "PS", accent: "#3b82f6" },
  ggpoker: { initials: "GG", accent: "#f59e0b" },
  partypoker: { initials: "PP", accent: "#a855f7" },
  "888poker": { initials: "888", accent: "#22c55e" },
};

function setStatus(node, message, kind) {
  node.innerHTML = "";
  if (!message) return;
  const icon =
    kind === "err"
      ? '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6"><circle cx="12" cy="12" r="10"/><path d="M12 8v5M12 16h.01"/></svg>'
      : kind === "ok"
        ? '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"/><path d="m22 4-10 10-3-3"/></svg>'
        : "";
  node.className = "status-msg" + (kind ? " " + kind : "");
  node.innerHTML = icon + `<span>${message}</span>`;
}

let rooms = [];
let selectedRooms = new Set();
let extraFolders = {};

function showScreen(loggedIn) {
  el("screen-login").classList.toggle("hidden", loggedIn);
  el("screen-app").classList.toggle("hidden", !loggedIn);
}

async function refreshConfig() {
  const cfg = await invoke("get_config");
  el("base-url").value = cfg.base_url ?? "";
  el("device-name-input").value = cfg.device_name ?? "";
  extraFolders = cfg.extra_folders ?? {};
  showScreen(cfg.logged_in);
  if (cfg.logged_in) {
    el("user-email").textContent = cfg.user_email ?? "(sem email)";
    el("account-avatar").textContent = (cfg.user_email ?? "?").trim().charAt(0).toUpperCase();
  }
  return cfg;
}

// ---------- Login (email/senha) ----------

el("toggle-password").addEventListener("click", () => {
  const input = el("password");
  input.type = input.type === "password" ? "text" : "password";
});

el("login-form").addEventListener("submit", async (e) => {
  e.preventDefault();
  const status = el("login-status");
  setStatus(status, "Entrando...");
  try {
    await invoke("login", { email: el("email").value, password: el("password").value });
    setStatus(status, "", null);
    await refreshConfig();
    await refreshAutostart();
    await loadRooms();
  } catch (err) {
    setStatus(status, String(err), "err");
  }
});

// ---------- Login com Google (abre o navegador do sistema) ----------

el("btn-google-login").addEventListener("click", async () => {
  const status = el("login-status");
  setStatus(status, "Abrindo o navegador...");
  try {
    await invoke("start_google_login");
  } catch (err) {
    setStatus(status, String(err), "err");
  }
});

// Caminho manual: o SO nem sempre sabe abrir pokersync-agent:// sozinho
// (varia por SO/instalação) — sem isso, quem confirma no Google e o app
// não reabre fica travado sem nenhuma saída.
el("btn-show-paste-link").addEventListener("click", () => {
  el("paste-link-row").classList.remove("hidden");
  el("btn-show-paste-link").classList.add("hidden");
  el("paste-link-input").focus();
});

el("btn-paste-link-confirm").addEventListener("click", async () => {
  const status = el("login-status");
  const link = el("paste-link-input").value.trim();
  if (!link) return;
  setStatus(status, "Confirmando login...");
  try {
    await invoke("paste_login_link", { link });
  } catch (err) {
    setStatus(status, String(err), "err");
  }
});

listen("google-login-result", async (event) => {
  const status = el("login-status");
  if (event.payload?.ok) {
    setStatus(status, "", null);
    await refreshConfig();
    await refreshAutostart();
    await loadRooms();
  } else {
    setStatus(status, event.payload?.error ?? "Não foi possível entrar com o Google.", "err");
  }
});

// ---------- Configurações avançadas (modal) ----------

function openSettings() {
  el("settings-overlay").classList.remove("hidden");
}
function closeSettings() {
  el("settings-overlay").classList.add("hidden");
}

el("btn-settings").addEventListener("click", openSettings);
el("btn-settings-close").addEventListener("click", closeSettings);
el("settings-overlay").addEventListener("click", (e) => {
  if (e.target === e.currentTarget) closeSettings(); // clique fora do card
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !el("settings-overlay").classList.contains("hidden")) closeSettings();
});

el("device-name-input").addEventListener("change", async (e) => {
  try {
    await invoke("save_device_name", { deviceName: e.target.value });
  } catch (err) {
    setStatus(el("config-status"), String(err), "err");
  }
});

el("base-url").addEventListener("change", async (e) => {
  try {
    await invoke("save_base_url", { baseUrl: e.target.value });
    await refreshConfig();
    setStatus(el("config-status"), "URL salva.", "ok");
  } catch (err) {
    setStatus(el("config-status"), String(err), "err");
  }
});

el("btn-test").addEventListener("click", async () => {
  const status = el("config-status");
  setStatus(status, "Testando...");
  try {
    const msg = await invoke("test_connection");
    setStatus(status, msg, "ok");
  } catch (err) {
    setStatus(status, String(err), "err");
  }
});

el("btn-logout").addEventListener("click", async () => {
  await invoke("logout");
  el("email").value = "";
  el("password").value = "";
  await refreshConfig();
});

async function refreshAutostart() {
  el("autostart-toggle").checked = await invoke("get_autostart");
}

el("autostart-toggle").addEventListener("change", async (e) => {
  const enabled = e.target.checked;
  try {
    await invoke("set_autostart", { enabled });
  } catch (err) {
    e.target.checked = !enabled;
    setStatus(el("config-status"), String(err), "err");
  }
});

// ---------- Salas ----------

function renderRoomFolders(room) {
  const list = document.querySelector(`.room-folders[data-slug="${room.slug}"]`);
  if (!list) return;
  list.innerHTML = "";
  const folders = extraFolders[room.slug] ?? [];
  if (folders.length === 0) {
    const empty = document.createElement("span");
    empty.className = "folder-chip empty";
    empty.textContent = "só pastas padrão";
    list.appendChild(empty);
    return;
  }
  for (const folder of folders) {
    const chip = document.createElement("span");
    chip.className = "folder-chip";
    chip.title = folder;
    const text = document.createElement("span");
    text.textContent = folder.length > 34 ? "…" + folder.slice(-32) : folder;
    const remove = document.createElement("button");
    remove.textContent = "×";
    remove.className = "chip-remove";
    remove.addEventListener("click", () => removeFolder(room.slug, folder));
    chip.appendChild(text);
    chip.appendChild(remove);
    list.appendChild(chip);
  }
}

async function saveFolders(slug) {
  await invoke("save_extra_folders", { room: slug, folders: extraFolders[slug] ?? [] });
}

async function addFolder(slug) {
  const picked = await openFolderDialog({ directory: true, multiple: false, title: "Escolher pasta de hand history" });
  if (!picked) return;
  const current = extraFolders[slug] ?? [];
  if (!current.includes(picked)) {
    extraFolders[slug] = [...current, picked];
    await saveFolders(slug);
  }
  renderRoomFolders(rooms.find((r) => r.slug === slug));
}

async function removeFolder(slug, folder) {
  extraFolders[slug] = (extraFolders[slug] ?? []).filter((f) => f !== folder);
  await saveFolders(slug);
  renderRoomFolders(rooms.find((r) => r.slug === slug));
}

async function loadRooms() {
  rooms = await invoke("list_rooms");
  const container = el("rooms");
  container.innerHTML = "";
  container.className = "rooms-grid";

  for (const room of rooms) {
    const style = ROOM_STYLE[room.slug] ?? { initials: room.display_name.slice(0, 2).toUpperCase(), accent: "#3b82f6" };

    const card = document.createElement("div");
    card.className = "room-card";
    card.style.setProperty("--acc", style.accent);
    card.dataset.slug = room.slug;

    const header = document.createElement("div");
    header.className = "room-header";

    const badge = document.createElement("div");
    badge.className = "room-badge";
    badge.textContent = style.initials;
    header.appendChild(badge);

    const nameWrap = document.createElement("div");
    nameWrap.className = "room-name-wrap";
    nameWrap.innerHTML = `<div class="room-name">${room.display_name}</div>`;
    header.appendChild(nameWrap);

    const toggleWrap = document.createElement("label");
    toggleWrap.className = "room-toggle";
    toggleWrap.title = "Incluir na busca/sincronização";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = true;
    selectedRooms.add(room.slug);
    checkbox.addEventListener("change", () => {
      if (checkbox.checked) selectedRooms.add(room.slug);
      else selectedRooms.delete(room.slug);
      card.classList.toggle("is-off", !checkbox.checked);
    });
    toggleWrap.appendChild(checkbox);
    header.appendChild(toggleWrap);

    card.appendChild(header);

    const folderList = document.createElement("div");
    folderList.className = "room-folders";
    folderList.dataset.slug = room.slug;
    card.appendChild(folderList);

    const addBtn = document.createElement("button");
    addBtn.className = "btn btn-ghost btn-sm";
    addBtn.textContent = "+ Adicionar pasta";
    addBtn.addEventListener("click", () => addFolder(room.slug));
    card.appendChild(addBtn);

    const scanStatus = document.createElement("div");
    scanStatus.className = "room-scan-status";
    scanStatus.dataset.slug = room.slug;
    card.appendChild(scanStatus);

    container.appendChild(card);
    renderRoomFolders(room);
  }
}

function roomScanStatusEl(slug) {
  return document.querySelector(`.room-scan-status[data-slug="${slug}"]`);
}

function renderResults(rows) {
  const table = el("results-table");
  const body = el("results-body");
  body.innerHTML = "";
  for (const row of rows) {
    const tr = document.createElement("tr");
    tr.innerHTML = `<td>${row.room}</td><td>${row.files}</td><td>${row.detail}</td>`;
    body.appendChild(tr);
  }
  table.classList.toggle("hidden", rows.length === 0);
}

el("btn-scan").addEventListener("click", async () => {
  const status = el("scan-status");
  setStatus(status, "Buscando hand histories no computador...");
  document.querySelectorAll(".room-scan-status").forEach((n) => (n.textContent = ""));
  try {
    const summaries = await invoke("scan_preview", { rooms: Array.from(selectedRooms) });
    renderResults(
      summaries.map((s) => ({
        room: rooms.find((r) => r.slug === s.room)?.display_name ?? s.room,
        files: s.files_found,
        detail: s.files_pending > 0 ? `${s.files_pending} novo(s)/alterado(s)` : "tudo sincronizado",
      }))
    );
    for (const s of summaries) {
      const node = roomScanStatusEl(s.room);
      if (!node) continue;
      node.textContent = s.files_pending > 0 ? `${s.files_pending} arquivo(s) pendente(s)` : `${s.files_found} arquivo(s), em dia`;
      node.classList.toggle("has-pending", s.files_pending > 0);
    }
    const total = summaries.reduce((acc, s) => acc + s.files_pending, 0);
    setStatus(status, `Busca concluída — ${total} arquivo(s) novo(s) ou alterado(s).`, "ok");
  } catch (err) {
    setStatus(status, String(err), "err");
  }
});

el("btn-sync").addEventListener("click", async () => {
  const status = el("scan-status");
  setStatus(status, "Sincronizando...");
  try {
    const summaries = await invoke("sync_now", { rooms: Array.from(selectedRooms) });
    renderResults(
      summaries.map((s) => ({
        room: rooms.find((r) => r.slug === s.room)?.display_name ?? s.room,
        files: s.files_synced,
        detail: `${s.imported} nova(s), ${s.duplicates} duplicada(s), ${s.errors} c/ erro`,
      }))
    );
    setStatus(status, "Sincronização concluída.", "ok");
  } catch (err) {
    setStatus(status, String(err), "err");
  }
});

(async function init() {
  const cfg = await refreshConfig();
  if (cfg.logged_in) {
    await refreshAutostart();
    await loadRooms();
  }
})();
