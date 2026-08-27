const invoke = window.__TAURI__.core.invoke;
const openFolderDialog = window.__TAURI__.dialog.open;

const el = (id) => document.getElementById(id);
const setStatus = (id, message, kind) => {
  const node = el(id);
  node.textContent = message ?? "";
  node.className = "status" + (kind ? " " + kind : "");
};

let rooms = [];
let selectedRooms = new Set();
let extraFolders = {}; // slug -> string[]

async function refreshConfig() {
  const cfg = await invoke("get_config");
  el("base-url").value = cfg.base_url ?? "";
  extraFolders = cfg.extra_folders ?? {};
  if (cfg.logged_in) {
    el("login-form").classList.add("hidden");
    el("logged-in").classList.remove("hidden");
    el("user-email").textContent = cfg.user_email ?? "(sem email)";
    el("device-name-pill").textContent = cfg.device_name ?? "";
  } else {
    el("login-form").classList.remove("hidden");
    el("logged-in").classList.add("hidden");
  }
  return cfg;
}

async function refreshAutostart() {
  el("autostart-toggle").checked = await invoke("get_autostart");
}

el("autostart-toggle").addEventListener("change", async (e) => {
  const enabled = e.target.checked;
  try {
    await invoke("set_autostart", { enabled });
  } catch (err) {
    e.target.checked = !enabled;
    setStatus("config-status", String(err), "err");
  }
});

function renderRoomFolders(room) {
  const list = document.querySelector(`.room-folders[data-slug="${room.slug}"]`);
  if (!list) return;
  list.innerHTML = "";
  const folders = extraFolders[room.slug] ?? [];
  if (folders.length === 0) {
    const empty = document.createElement("span");
    empty.className = "pill";
    empty.textContent = "só pastas padrão";
    list.appendChild(empty);
    return;
  }
  for (const folder of folders) {
    const chip = document.createElement("span");
    chip.className = "folder-chip";
    chip.title = folder;
    const text = document.createElement("span");
    text.textContent = folder.length > 42 ? "…" + folder.slice(-40) : folder;
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
  for (const room of rooms) {
    const card = document.createElement("div");
    card.className = "room-card";

    const header = document.createElement("label");
    header.className = "room-header";
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = true;
    checkbox.dataset.slug = room.slug;
    checkbox.addEventListener("change", () => {
      if (checkbox.checked) selectedRooms.add(room.slug);
      else selectedRooms.delete(room.slug);
    });
    selectedRooms.add(room.slug);
    header.appendChild(checkbox);
    header.append(room.display_name);
    header.title = "Pastas padrão verificadas:\n" + room.default_folders.join("\n");
    card.appendChild(header);

    const folderList = document.createElement("div");
    folderList.className = "room-folders";
    folderList.dataset.slug = room.slug;
    card.appendChild(folderList);

    const addBtn = document.createElement("button");
    addBtn.className = "secondary small";
    addBtn.textContent = "+ Adicionar pasta";
    addBtn.addEventListener("click", () => addFolder(room.slug));
    card.appendChild(addBtn);

    container.appendChild(card);
    renderRoomFolders(room);
  }
}

function renderResults(rows, columns) {
  const table = el("results-table");
  const body = el("results-body");
  body.innerHTML = "";
  for (const row of rows) {
    const tr = document.createElement("tr");
    for (const col of columns) {
      const td = document.createElement("td");
      td.textContent = row[col] ?? "";
      tr.appendChild(td);
    }
    body.appendChild(tr);
  }
  table.classList.toggle("hidden", rows.length === 0);
}

el("btn-save-url").addEventListener("click", async () => {
  try {
    await invoke("save_base_url", { baseUrl: el("base-url").value });
    await refreshConfig(); // reflete a URL normalizada (ex.: https:// completado)
    setStatus("config-status", "URL salva.", "ok");
  } catch (e) {
    setStatus("config-status", String(e), "err");
  }
});

el("btn-test").addEventListener("click", async () => {
  setStatus("config-status", "Testando...");
  try {
    const msg = await invoke("test_connection");
    setStatus("config-status", msg, "ok");
  } catch (e) {
    setStatus("config-status", String(e), "err");
  }
});

el("btn-login").addEventListener("click", async () => {
  setStatus("login-status", "Entrando...");
  try {
    await invoke("login", { email: el("email").value, password: el("password").value });
    setStatus("login-status", "Login OK.", "ok");
    await refreshConfig();
  } catch (e) {
    setStatus("login-status", String(e), "err");
  }
});

el("btn-logout").addEventListener("click", async () => {
  await invoke("logout");
  await refreshConfig();
});

el("btn-scan").addEventListener("click", async () => {
  setStatus("scan-status", "Buscando hand histories no computador...");
  try {
    const summaries = await invoke("scan_preview", { rooms: Array.from(selectedRooms) });
    renderResults(
      summaries.map((s) => ({ room: s.room, files_found: s.files_found, files_pending: s.files_pending })),
      ["room", "files_found", "files_pending"]
    );
    const total = summaries.reduce((acc, s) => acc + s.files_pending, 0);
    setStatus("scan-status", `Busca concluída — ${total} arquivo(s) novo(s) ou alterado(s).`, "ok");
  } catch (e) {
    setStatus("scan-status", String(e), "err");
  }
});

el("btn-sync").addEventListener("click", async () => {
  setStatus("scan-status", "Sincronizando...");
  try {
    const summaries = await invoke("sync_now", { rooms: Array.from(selectedRooms) });
    renderResults(
      summaries.map((s) => ({
        room: s.room,
        files_found: s.files_synced,
        files_pending: `${s.imported} novas, ${s.duplicates} duplicadas, ${s.errors} c/ erro`,
      })),
      ["room", "files_found", "files_pending"]
    );
    setStatus("scan-status", "Sincronização concluída.", "ok");
  } catch (e) {
    setStatus("scan-status", String(e), "err");
  }
});

(async function init() {
  await refreshConfig();
  await refreshAutostart();
  await loadRooms();
})();
