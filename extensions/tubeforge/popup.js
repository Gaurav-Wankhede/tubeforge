// TubeForge popup — server status + quick keyword research.

const statusEl = document.getElementById("status");
const kwEl = document.getElementById("kw");
const goEl = document.getElementById("go");
const resultEl = document.getElementById("result");

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

chrome.runtime.sendMessage({ type: "tf:status" }, (r) => {
  if (r?.ok) {
    statusEl.textContent = "online";
    statusEl.className = "status online";
  } else {
    statusEl.textContent = "offline";
    statusEl.className = "status offline";
  }
});

goEl.addEventListener("click", research);
kwEl.addEventListener("keydown", (e) => {
  if (e.key === "Enter") research();
});

async function research() {
  const q = kwEl.value.trim();
  if (!q) return;
  resultEl.innerHTML = `<div class="row">Searching…</div>`;
  chrome.runtime.sendMessage(
    { type: "tf:fetch", path: `/api/keywords/inspect?q=${encodeURIComponent(q)}&serp=4` },
    (r) => {
      if (!r?.ok) {
        resultEl.innerHTML = `<div class="error">${esc(r?.error || "failed")}</div>`;
        return;
      }
      const d = r.data;
      const tags = (d.suggested_tags || []).slice(0, 8);
      resultEl.innerHTML = `
        <div class="row"><span>Keyword score</span><b>${(d.keyword_score ?? 0).toFixed(0)}/100</b></div>
        <div class="row"><span>Opportunity</span><b>${(d.opportunity_score ?? 0).toFixed(0)}</b></div>
        <div class="row"><span>Competition</span><b>${(d.competition_score ?? 0).toFixed(0)}</b></div>
        <div class="row"><span>Volume</span><b>${esc(d.volume_label ?? "?")} (avg ${fmtViews(d.serp_mean_views)})</b></div>
        <div class="row"><span>Channels ranking</span><b>${d.ranking_channels ?? 0}</b></div>
        <div class="row"><span>Active (90d)</span><b>${d.actively_published ? "yes" : "no"} · ${d.recent_uploads ?? 0} new</b></div>
        ${d.verdict ? `<div class="verdict">${esc(d.verdict)}</div>` : ""}
        ${tags.length ? `<div class="row" style="margin-top:6px"><span>Tags</span><b>${tags.map((t) => esc(t.tag ?? t)).join(", ")}</b></div>` : ""}
      `;
    },
  );
}
