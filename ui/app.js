const invoke = window.__TAURI__.core.invoke;

const el = (id) => document.getElementById(id);
const setStatus = (id, message, kind) => {
  const node = el(id);
  node.textContent = message ?? "";
  node.className = "status" + (kind ? " " + kind : "");
};

let rooms = [];
let selectedRooms = new Set();

async function refreshConfig() {
  const cfg = await invoke("get_config");
  el("base-url").value = cfg.base_url ?? "";
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

async function loadRooms() {
  rooms = await invoke("list_rooms");
  const container = el("rooms");
  container.innerHTML = "";
  for (const room of rooms) {
    const label = document.createElement("label");
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = true;
    checkbox.dataset.slug = room.slug;
    checkbox.addEventListener("change", () => {
      if (checkbox.checked) selectedRooms.add(room.slug);
      else selectedRooms.delete(room.slug);
    });
    selectedRooms.add(room.slug);
    label.appendChild(checkbox);
    label.append(room.display_name);
    container.appendChild(label);
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
  await loadRooms();
})();
