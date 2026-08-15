import { invoke } from "https://unpkg.com/@tauri-apps/api@2/core";
const $ = s => document.querySelector(s), savedProvider = (localStorage.provider || "moviebox").toLowerCase(), state = { provider: ["moviebox", "fourkhdhub", "circleftp"].includes(savedProvider) ? savedProvider : "moviebox", details: null };
const providerNames = { moviebox: "MovieBox", fourkhdhub: "4KHDHub", circleftp: "CircleFTP" };
function updateWelcomeSubtitle() {
    const name = providerNames[state.provider] || "MovieBox";
    const el = $("#welcomeSubtitle");
    if (el) el.textContent = `Search ${name} or choose another provider in settings.`;
}
$("#provider").value = state.provider;
updateWelcomeSubtitle();

/* ── navigation history ── */
const history = [], historyData = [];
let historyIndex = -1, navigating = false;
function pushNav(view, data) { if (!navigating) { history.splice(historyIndex + 1); historyData.splice(historyIndex + 1); history.push(view); historyData.push(data); historyIndex = history.length - 1 } updateNavButtons() }
function showView(view, data) { $("#welcome").classList.toggle("hidden", view !== "welcome"); $("#results").classList.toggle("hidden", view !== "results"); $("#detail").classList.toggle("hidden", view !== "detail"); if (view === "results" && data?.html) { $("#results").innerHTML = data.html; bindResultCards(data.items) } if (view === "detail" && data?.html) { $("#detail").innerHTML = data.html; bindDetailActions(data.detail) } }
function updateNavButtons() { $("#navBack").disabled = historyIndex <= 0; $("#navForward").disabled = historyIndex >= history.length - 1 }
$("#navBack").onclick = () => { if (historyIndex <= 0) return; historyIndex--; navigating = true; showView(history[historyIndex], historyData[historyIndex]); navigating = false; updateNavButtons() };
$("#navForward").onclick = () => { if (historyIndex >= history.length - 1) return; historyIndex++; navigating = true; showView(history[historyIndex], historyData[historyIndex]); navigating = false; updateNavButtons() };
pushNav("welcome", null);

/* ── typewriter placeholder ── */
{
    const suggestions = [
        'Try "Breaking Bad"',
        'Try "Insidious: The Red Door"',
        'Try "Death Note"',
        'Try "From"',
        'Try "Interstellar"',
        'Try "Dhurandhar"',
        'Try "The Last of Us"',
        'Try "12th Fail"',
        'Try "Oppenheimer"',
        'Try "Game of Thrones"',
        'Try "Peaky Blinders"',
        'Try "Friends"',
        'Try "The Office"',
        'Try "Sherlock"',
        'Try "Stranger Things"',
        'Try "Naruto"',
        'Try "Attack on Titan"',
        'Try "One Piece"',
        'Try "Demon Slayer"',
        'Try "My Hero Academia"',
        'Try "Black Clover"',
        'Try "Tokyo Ghoul"',
        'Try "Fullmetal Alchemist"',
        'Try "Sword Art Online"',
        'Try "Hunter x Hunter"',
        'Try "One Punch Man"',

    ];
    const tw = $("#typewriter"), input = $("#query"), wrap = $(".search-wrap");
    let idx = 0, running = true;
    const sleep = ms => new Promise(r => setTimeout(r, ms));

    input.addEventListener("focus", () => wrap.classList.add("has-focus"));
    input.addEventListener("blur", () => wrap.classList.remove("has-focus"));
    input.addEventListener("input", () => {
        wrap.classList.toggle("has-value", input.value.length > 0);
    });

    (async () => {
        while (running) {
            const text = suggestions[idx % suggestions.length];
            // type in
            for (let i = 0; i <= text.length; i++) {
                tw.textContent = text.slice(0, i);
                await sleep(55);
            }
            await sleep(1800);
            // erase
            for (let i = text.length; i >= 0; i--) {
                tw.textContent = text.slice(0, i);
                await sleep(30);
            }
            await sleep(350);
            idx++;
        }
    })();
}

/* ── utilities ── */
function toast(message) { const e = $("#toast"); e.textContent = message; e.classList.remove("hidden"); setTimeout(() => e.classList.add("hidden"), 4200) }
function esc(v) { return String(v ?? "").replace(/[&<>'"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", "'": "&#39;", '"': "&quot;" }[c])) }
function poster(item) { return item.poster ? `style="background-image:linear-gradient(0deg,#0008,transparent 40%),url('${item.poster}')"` : ""; }

function formatSize(bytes) {
    if (bytes == null) return "Unknown size";
    if (bytes >= 1073741824) return (bytes / 1073741824).toFixed(1) + " GB";
    if (bytes >= 1048576) return (bytes / 1048576).toFixed(0) + " MB";
    if (bytes >= 1024) return (bytes / 1024).toFixed(0) + " KB";
    return bytes + " B";
}

function formatSpeed(bps) {
    const n = Number(bps) || 0;
    if (n >= 1048576) return (n / 1048576).toFixed(1) + " MB/s";
    if (n >= 1024) return (n / 1024).toFixed(0) + " KB/s";
    return n.toFixed(0) + " B/s";
}

/* ── search results ── */
function resultsHtml(items) { return items.length ? items.map((x, i) => `<button class="card" data-index="${i}"><div class="poster" ${poster(x)}>${x.poster ? "" : "No poster"}</div><h3>${esc(x.title)}</h3><div class="meta">${esc(x.year)} · ${esc(x.kind)}</div></button>`).join("") : "<p class='hint'>No results found.</p>" }
function bindResultCards(items) { $("#results").querySelectorAll(".card").forEach(b => b.onclick = () => details(items[+b.dataset.index])) }

/* ── detail view ── */
function detailHtml(d) {
    let episodesBlock = "";
    if (d.episodes.length) {
        const seasons = new Map();
        for (const e of d.episodes) {
            if (!seasons.has(e.season)) seasons.set(e.season, []);
            seasons.get(e.season).push(e);
        }
        const seasonBlocks = [...seasons.entries()].map(([num, eps]) =>
            `<div class="season-block"><h3 class="season-title">Season ${num}</h3><div class="episode-grid">${eps.map(e => `<button class="episode" data-se="${e.season}" data-ep="${e.episode}">Episode ${e.episode}</button>`).join("")}</div></div>`
        ).join("");
        episodesBlock = `<div class="episodes"><h2>Episodes</h2>${seasonBlocks}</div>`;
    }
    return `<div class="hero"><div class="poster" ${poster(d)}>${d.poster ? "" : "No poster"}</div><div><div class="badges">${esc(d.kind.toUpperCase())} · ${esc(d.year)} · ${esc(d.rating)}</div><h1>${esc(d.title)}</h1><div class="meta">${d.genres.map(esc).join(" · ")}</div><p class="copy">${esc(d.synopsis)}</p>${d.kind !== "series" ? '<div class="actions"><button id="playMovie">Select stream</button></div>' : ""}</div></div>${episodesBlock}`;
}
function bindDetailActions(d) { state.details = d; $("#playMovie")?.addEventListener("click", () => streams()); $("#detail").querySelectorAll(".episode").forEach(b => b.onclick = () => streams(+b.dataset.se, +b.dataset.ep)) }

/* ── search ── */
async function search() { const query = $("#query").value.trim(); if (!query) return; $("#welcome").classList.add("hidden"); $("#detail").classList.add("hidden"); const box = $("#results"); box.classList.remove("hidden"); box.innerHTML = "<p class='hint'>Searching…</p>"; try { const items = await invoke("search", { provider: state.provider, query }); const html = resultsHtml(items); box.innerHTML = html; bindResultCards(items); pushNav("results", { html, items }) } catch (e) { box.innerHTML = ""; toast(String(e)) } }

/* ── details ── */
async function details(item) { $("#results").classList.add("hidden"); const box = $("#detail"); box.classList.remove("hidden"); box.innerHTML = "<p class='hint'>Loading details…</p>"; try { const d = await invoke("get_details", { provider: state.provider, id: item.id }); const html = detailHtml(d); box.innerHTML = html; bindDetailActions(d); pushNav("detail", { html, detail: d }) } catch (e) { toast(String(e)); $("#welcome").classList.remove("hidden"); box.classList.add("hidden") } }

/* ── streams (with Play + Download buttons) ── */
async function streams(season = null, episode = null) {
    const d = state.details;
    const episodeLabel = (season != null && episode != null) ? `S${String(season).padStart(2, '0')}E${String(episode).padStart(2, '0')}` : null;
    $("#streamModal").classList.remove("hidden");
    $("#streamList").innerHTML = "<p class='hint'>Finding available streams…</p>";
    try {
        const list = await invoke("get_streams", { provider: state.provider, id: d.id, season, episode });
        if (!list.length) {
            $("#streamList").innerHTML = "<p class='hint'>No playable stream was returned.</p>";
            return;
        }
        $("#streamList").innerHTML = list.map((s, i) => `
            <div class="stream-entry" data-i="${i}">
                <div class="stream-info">
                    <b class="stream-res">${esc(s.resolution)}</b>
                    <span class="stream-label">${esc(s.label)}</span>
                </div>
                <div class="stream-actions">
                    <button class="stream-play" data-i="${i}">▶ Play in VLC</button>
                    <button class="stream-download" data-i="${i}">⬇ Download (${formatSize(s.sizeBytes)})</button>
                </div>
            </div>
        `).join("");

        // Play in VLC buttons
        $("#streamList").querySelectorAll(".stream-play").forEach(b => {
            b.onclick = async () => {
                try {
                    const s = list[+b.dataset.i];
                    await invoke("play_in_vlc", { url: s.url, headers: s.headers });
                    $("#streamModal").classList.add("hidden");
                    toast("Opening VLC…");
                } catch (e) { toast(String(e)); }
            };
        });

        // Download buttons
        $("#streamList").querySelectorAll(".stream-download").forEach(b => {
            b.onclick = async () => {
                try {
                    const s = list[+b.dataset.i];
                    await invoke("start_download", {
                        title: d.title,
                        episodeLabel,
                        resolution: s.resolution,
                        url: s.url,
                        headers: s.headers
                    });
                    $("#streamModal").classList.add("hidden");
                    toast("Download started: " + d.title + (episodeLabel ? " " + episodeLabel : ""));
                } catch (e) { toast(String(e)); }
            };
        });
    } catch (e) {
        $("#streamModal").classList.add("hidden");
        toast(String(e));
    }
}

/* ── downloads panel ── */
let downloadPollInterval = null;

function openDownloadsPanel() {
    $("#downloadModal").classList.remove("hidden");
    refreshDownloads();
    downloadPollInterval = setInterval(refreshDownloads, 1000);
}

function closeDownloadsPanel() {
    if (downloadPollInterval) {
        clearInterval(downloadPollInterval);
        downloadPollInterval = null;
    }
}

async function refreshDownloads() {
    try {
        const downloads = await invoke("get_downloads");
        const container = $("#downloadList");
        if (!downloads.length) {
            container.innerHTML = "<p class='hint'>No downloads yet.</p>";
            return;
        }
        container.innerHTML = downloads.map(dl => renderDownloadEntry(dl)).join("");
        bindDownloadActions();
    } catch (e) { /* silently ignore poll errors */ }
}

function renderDownloadEntry(dl) {
    const s = dl.status;
    let statusHtml = "";
    let actionsHtml = "";
    let progressHtml = "";

    switch (s.kind) {
        case "queued":
            statusHtml = `<span class="dl-status status-queued">Queued</span>`;
            actionsHtml = `<button class="dl-btn dl-cancel" data-id="${esc(dl.id)}">Cancel</button>`;
            break;
        case "downloading": {
            const pct = s.total ? Math.round((s.downloaded / s.total) * 100) : null;
            const pctText = pct != null ? `${pct}%` : "";
            const sizeText = `${formatSize(s.downloaded)}${s.total ? " / " + formatSize(s.total) : ""}`;
            const barWidth = pct != null ? `${pct}%` : "100%";
            const barClass = pct != null ? "dl-progress-fill" : "dl-progress-fill indeterminate";
            statusHtml = `<span class="dl-status status-downloading">⬇ Downloading ${pctText}</span>`;
            progressHtml = `<div class="dl-progress"><div class="${barClass}" style="width:${barWidth}"></div></div>`;
            actionsHtml = `
                <span class="dl-speed">${formatSpeed(s.speedBps ?? s.speed_bps)} · ${sizeText}</span>
                <button class="dl-btn dl-pause" data-id="${esc(dl.id)}">Pause</button>
                <button class="dl-btn dl-cancel" data-id="${esc(dl.id)}">Cancel</button>
            `;
            break;
        }
        case "paused": {
            const sizeText = `${formatSize(s.downloaded)}${s.total ? " / " + formatSize(s.total) : ""}`;
            const pct = s.total ? Math.round((s.downloaded / s.total) * 100) : 0;
            statusHtml = `<span class="dl-status status-paused">⏸ Paused</span>`;
            progressHtml = `<div class="dl-progress"><div class="dl-progress-fill paused" style="width:${s.total ? pct + '%' : '0%'}"></div></div>`;
            actionsHtml = `
                <span class="dl-speed">${sizeText}</span>
                <button class="dl-btn dl-resume" data-id="${esc(dl.id)}">Resume</button>
                <button class="dl-btn dl-cancel" data-id="${esc(dl.id)}">Cancel</button>
            `;
            break;
        }
        case "completed":
            statusHtml = `<span class="dl-status status-completed">Completed · ${formatSize(s.size)}</span>`;
            actionsHtml = `
                <button class="dl-btn dl-open" data-path="${esc(dl.filePath)}">Open in Explorer</button>
                <button class="dl-btn dl-remove" data-id="${esc(dl.id)}">Remove</button>
            `;
            break;
        case "failed":
            statusHtml = `<span class="dl-status status-failed">Failed: ${esc(s.error)}</span>`;
            actionsHtml = `
                <button class="dl-btn dl-retry" data-id="${esc(dl.id)}">Retry</button>
                <button class="dl-btn dl-remove" data-id="${esc(dl.id)}">Remove</button>
            `;
            break;
        case "cancelled":
            statusHtml = `<span class="dl-status status-cancelled">Cancelled</span>`;
            actionsHtml = `<button class="dl-btn dl-remove" data-id="${esc(dl.id)}">Remove</button>`;
            break;
    }

    const epLabel = dl.episodeLabel ? ` · ${esc(dl.episodeLabel)}` : "";
    return `
        <div class="download-entry">
            <div class="dl-header">
                <div class="dl-title">${esc(dl.title)}${epLabel} <span class="dl-res">[${esc(dl.resolution)}]</span></div>
                ${statusHtml}
            </div>
            ${progressHtml}
            <div class="dl-actions">${actionsHtml}</div>
        </div>
    `;
}

function bindDownloadActions() {
    document.querySelectorAll(".dl-pause").forEach(b => b.onclick = async () => { try { await invoke("pause_download", { id: b.dataset.id }); refreshDownloads(); } catch (e) { toast(String(e)); } });
    document.querySelectorAll(".dl-resume").forEach(b => b.onclick = async () => { try { await invoke("resume_download", { id: b.dataset.id }); refreshDownloads(); } catch (e) { toast(String(e)); } });
    document.querySelectorAll(".dl-cancel").forEach(b => b.onclick = async () => { try { await invoke("cancel_download", { id: b.dataset.id }); refreshDownloads(); } catch (e) { toast(String(e)); } });
    document.querySelectorAll(".dl-remove").forEach(b => b.onclick = async () => { try { await invoke("remove_download", { id: b.dataset.id }); refreshDownloads(); } catch (e) { toast(String(e)); } });
    document.querySelectorAll(".dl-retry").forEach(b => b.onclick = async () => { try { await invoke("retry_download", { id: b.dataset.id }); refreshDownloads(); } catch (e) { toast(String(e)); } });
    document.querySelectorAll(".dl-open").forEach(b => b.onclick = async () => { try { await invoke("open_download_location", { path: b.dataset.path }); } catch (e) { toast(String(e)); } });
}

/* ── event bindings ── */
$("#searchForm").onsubmit = e => { e.preventDefault(); search() };
$("#settings").onclick = () => $("#modal").classList.remove("hidden");
$("#downloads").onclick = () => openDownloadsPanel();
$("#save").onclick = () => { state.provider = $("#provider").value; localStorage.provider = state.provider; updateWelcomeSubtitle(); $("#modal").classList.add("hidden"); toast(`${providerNames[state.provider] || 'MovieBox'} selected`) };
document.querySelectorAll("[data-close]").forEach(b => b.onclick = () => { b.closest(".modal").classList.add("hidden"); closeDownloadsPanel(); });
$("#home").onclick = e => { e.preventDefault(); showView("welcome", null); pushNav("welcome", null) };
