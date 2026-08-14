// TubeForge content script — VidIQ-style overlay for YouTube.
//
// - Watch pages (`/watch`): a draggable panel with the video's TubeForge
//   SEO score, tags, performance signals, and a keyword-research box.
// - Search results (`/results`): score badges on video cards (for videos
//   already scored in the local DB).
// - Channel pages (`/@handle`): a one-line channel audit score.
//
// All data comes from the local server via the service-worker proxy
// (host_permissions bypass CORS; the loopback server stays unchanged).

(() => {
  if (window.__tfInjected) return;
  window.__tfInjected = true;

  const API = {
    fetch(path) {
      return new Promise((resolve, reject) => {
        chrome.runtime.sendMessage({ type: "tf:fetch", path }, (r) => {
          if (chrome.runtime.lastError) return reject(chrome.runtime.lastError);
          if (r?.ok) resolve(r.data);
          else reject(new Error(r?.error || "fetch failed"));
        });
      });
    },
    status() {
      return new Promise((resolve) => {
        chrome.runtime.sendMessage({ type: "tf:status" }, (r) => {
          resolve(Boolean(r?.ok));
        });
      });
    },
  };

  const root = document.createElement("div");
  root.id = "tf-overlay-root";
  root.innerHTML = `
    <div class="tf-header tf-drag-handle">
      <div class="tf-logo">Tube<span>Forge</span></div>
      <div class="tf-status" id="tf-status">…</div>
    </div>
    <div class="tf-body" id="tf-body">
      <div class="tf-error" id="tf-error" style="display:none"></div>
      <div class="tf-loading" id="tf-loading">Loading video analysis…</div>
      <div id="tf-content" style="display:none"></div>
    </div>
  `;
  document.documentElement.appendChild(root);

  const $ = (id) => root.querySelector(id);
  const statusEl = $("#tf-status");
  const errorEl = $("#tf-error");
  const loadingEl = $("#tf-loading");
  const contentEl = $("#tf-content");

  function showError(msg) {
    errorEl.textContent = msg;
    errorEl.style.display = "block";
  }
  function hideError() {
    errorEl.style.display = "none";
  }

  function scoreClass(v) {
    if (v >= 80) return "tf-score-good";
    if (v >= 60) return "tf-score-mid";
    if (v >= 40) return "tf-score-bad";
    return "tf-score-bad";
  }

  function fmtViews(v) {
    if (v === null || v === undefined) return "—";
    if (v >= 1_000_000) return (v / 1_000_000).toFixed(1) + "M";
    if (v >= 1_000) return (v / 1_000).toFixed(1) + "k";
    return String(v);
  }

  function esc(s) {
    const d = document.createElement("div");
    d.textContent = String(s ?? "");
    return d.innerHTML;
  }

  // --- Watch page panel ---------------------------------------------------
  async function renderWatch(videoId) {
    hideError();
    loadingEl.style.display = "block";
    contentEl.style.display = "none";
    try {
      const d = await API.fetch(`/api/scores/${videoId}`);
      const perf = d.performance || {};

      // Tags: prefer the normalized tags table; fall back to the video
      // row's own tags (RSS/API ingest stores them in videos.tags).
      let tags = [];
      const tagList = await API.fetch(`/api/tags/video/${videoId}`).catch(() => null);
      if (tagList?.tags?.length) {
        tags = tagList.tags.map((t) => t.name);
      } else {
        const all = await API.fetch("/api/videos?page_size=200").catch(() => null);
        const hit = (all?.items || []).find((x) => x.video_id === videoId);
        if (hit?.tags?.length) tags = hit.tags;
      }

      const perfRows = [
        perf.vph != null && `<div class="tf-row"><span>Views/hour</span><b>${perf.vph.toFixed(1)}</b></div>`,
        perf.engagement_score != null && `<div class="tf-row"><span>Engagement</span><b>${perf.engagement_score.toFixed(0)}/100</b></div>`,
        perf.retention_score != null && `<div class="tf-row"><span>Retention</span><b>${perf.retention_score.toFixed(0)}/100</b></div>`,
        perf.trending && `<div class="tf-row"><span>Trending</span><b class="tf-score-good">yes 🔥</b></div>`,
      ].filter(Boolean).join("");

      contentEl.innerHTML = `
        <div class="tf-section">
          <h4>SEO score</h4>
          <div class="tf-grid">
            <div class="tf-stat"><div class="v ${scoreClass(d.total)}">${d.total.toFixed(0)}</div><div class="k">Total</div></div>
            <div class="tf-stat"><div class="v ${scoreClass(d.seo_total)}">${d.seo_total.toFixed(0)}</div><div class="k">SEO</div></div>
            <div class="tf-stat"><div class="v ${scoreClass(d.geo_total)}">${d.geo_total.toFixed(0)}</div><div class="k">GEO</div></div>
          </div>
        </div>
        ${perfRows ? `<div class="tf-section"><h4>Performance</h4>${perfRows}</div>` : ""}
        ${tags.length ? `<div class="tf-section"><h4>Tags (${tags.length})</h4><div class="tf-tags">${tags.slice(0, 12).map((t) => `<span class="tf-tag">${esc(t)}</span>`).join("")}</div></div>` : ""}
        <div class="tf-section">
          <h4>Keyword research</h4>
          <input class="tf-input" id="tf-kw" placeholder="Type a topic… (e.g. rust async)">
          <button class="tf-btn" id="tf-research">Research</button>
          <div id="tf-kw-result"></div>
        </div>
        <div class="tf-section">
          <a class="tf-link" target="_blank" href="http://127.0.0.1:17487/scores/${videoId}">Open in TubeForge dashboard →</a>
        </div>
      `;

      const kwInput = $("#tf-kw");
      const kwBtn = $("#tf-research");
      const kwResult = $("#tf-kw-result");
      kwBtn.addEventListener("click", () => research(kwInput.value, kwResult));
      kwInput.addEventListener("keydown", (e) => {
        if (e.key === "Enter") research(kwInput.value, kwResult);
      });

      loadingEl.style.display = "none";
      contentEl.style.display = "block";
    } catch (e) {
      loadingEl.style.display = "none";
      showError(`Could not load analysis (${e.message}). Is \`tubeforge serve\` running?`);
    }
  }

  async function research(query, el) {
    const q = query.trim();
    if (!q) return;
    el.innerHTML = `<div class="tf-loading">Searching YouTube for "${esc(q)}"…</div>`;
    try {
      const r = await API.fetch(`/api/keywords/inspect?q=${encodeURIComponent(q)}&serp=4`);
      const score = r.keyword_score ?? 0;
      const tags = (r.suggested_tags || []).slice(0, 8);
      el.innerHTML = `
        <div class="tf-grid" style="margin-top:8px">
          <div class="tf-stat"><div class="v ${scoreClass(score)}">${score.toFixed(0)}</div><div class="k">Score</div></div>
          <div class="tf-stat"><div class="v ${scoreClass(r.opportunity_score ?? 0)}">${(r.opportunity_score ?? 0).toFixed(0)}</div><div class="k">Opp.</div></div>
          <div class="tf-stat"><div class="v ${scoreClass(100 - (r.competition_score ?? 0))}">${(r.competition_score ?? 0).toFixed(0)}</div><div class="k">Comp.</div></div>
        </div>
        ${r.verdict ? `<div class="tf-verdict" style="margin-top:8px">${esc(r.verdict)}</div>` : ""}
        ${tags.length ? `<div class="tf-tags" style="margin-top:8px">${tags.map((t) => `<span class="tf-tag">${esc(t.tag ?? t)}${t.usage ? ` ×${t.usage}` : ""}</span>`).join("")}</div>` : ""}
        <div class="tf-row" style="margin-top:8px">
          <span>Volume</span><b>${esc(r.volume_label ?? "?")}</b>
        </div>
        <div class="tf-row">
          <span>Active</span><b>${r.actively_published ? "yes" : "no"} (${r.recent_uploads ?? 0} new/90d)</b>
        </div>
      `;
    } catch (e) {
      el.innerHTML = `<div class="tf-error" style="margin-top:8px">${esc(e.message)}</div>`;
    }
  }

  // --- Search result badges ------------------------------------------------
  async function badgeSearchResults() {
    const cards = document.querySelectorAll("ytd-video-renderer, ytd-rich-item-renderer");
    if (!cards.length) return;
    // Fetch scored videos once (paginated API).
    let items = [];
    try {
      const d = await API.fetch("/api/videos?page_size=200");
      items = d.items || [];
    } catch {
      return;
    }
    const byId = new Map(items.map((v) => [v.video_id, v.total_score]));

    cards.forEach((card) => {
      const a = card.querySelector("a#thumbnail, a#video-title-link, a[href*='/watch']");
      const href = a?.getAttribute("href") || "";
      const m = href.match(/v=([A-Za-z0-9_-]{11})/);
      if (!m) return;
      const score = byId.get(m[1]);
      if (score === undefined) return;
      const thumb = card.querySelector("ytd-thumbnail, #thumbnail");
      if (!thumb || thumb.querySelector(".tf-badge")) return;
      const badge = document.createElement("div");
      badge.className = `tf-badge ${score >= 80 ? "tf-badge-good" : score >= 60 ? "tf-badge-mid" : "tf-badge-bad"}`;
      badge.textContent = `TF ${score.toFixed(0)}`;
      thumb.style.position = "relative";
      thumb.appendChild(badge);
    });
  }

  // --- Channel audit badge -------------------------------------------------
  async function badgeChannel() {
    const m = location.pathname.match(/^\/@([^/]+)/);
    if (!m) return;
    try {
      const audits = await API.fetch("/api/audit");
      if (!Array.isArray(audits)) return;
      const a = audits.find((x) => x.channel_name.toLowerCase().includes(m[1].toLowerCase()));
      if (!a) return;
      const header = document.querySelector("ytd-c4-tabbed-header-renderer, #page-header");
      if (!header || header.querySelector(".tf-badge")) return;
      const badge = document.createElement("div");
      badge.className = `tf-badge ${a.total_score >= 70 ? "tf-badge-good" : a.total_score >= 50 ? "tf-badge-mid" : "tf-badge-bad"}`;
      badge.textContent = `TF Audit ${a.total_score.toFixed(0)} (${a.grade})`;
      badge.style.position = "absolute";
      badge.style.top = "12px";
      badge.style.right = "12px";
      header.style.position = "relative";
      header.appendChild(badge);
    } catch {
      /* channel not audited / server off */
    }
  }

  // --- Routing + status ----------------------------------------------------
  async function route() {
    const ok = await API.status();
    statusEl.textContent = ok ? "online" : "offline";
    statusEl.className = `tf-status ${ok ? "online" : "offline"}`;

    const path = location.pathname;
    if (path.startsWith("/watch")) {
      const m = location.search.match(/[?&]v=([A-Za-z0-9_-]{11})/);
      if (m) {
        renderWatch(m[1]);
      } else {
        loadingEl.style.display = "none";
        contentEl.style.display = "none";
      }
    } else if (path.startsWith("/results")) {
      loadingEl.style.display = "none";
      contentEl.style.display = "none";
      // Search badges run on a small delay so YouTube's virtual list settles.
      setTimeout(badgeSearchResults, 800);
      const observer = new MutationObserver(() => setTimeout(badgeSearchResults, 300));
      observer.observe(document.body, { childList: true, subtree: true });
      setTimeout(() => observer.disconnect(), 20_000);
    } else if (path.startsWith("/@")) {
      loadingEl.style.display = "none";
      contentEl.style.display = "none";
      badgeChannel();
    } else {
      loadingEl.style.display = "none";
      contentEl.style.display = "none";
    }
  }

  // --- Dragging ------------------------------------------------------------
  let drag = null;
  const header = root.querySelector(".tf-header");
  header.addEventListener("mousedown", (e) => {
    drag = { x: e.clientX - root.offsetLeft, y: e.clientY - root.offsetTop };
    e.preventDefault();
  });
  document.addEventListener("mousemove", (e) => {
    if (!drag) return;
    root.style.left = `${e.clientX - drag.x}px`;
    root.style.top = `${e.clientY - drag.y}px`;
    root.style.right = "auto";
  });
  document.addEventListener("mouseup", () => (drag = null));

  // SPA navigation within YouTube — re-route on path change.
  let lastPath = location.pathname + location.search;
  new MutationObserver(() => {
    const p = location.pathname + location.search;
    if (p !== lastPath) {
      lastPath = p;
      route();
    }
  }).observe(document.body, { childList: true, subtree: true });

  route();
})();
