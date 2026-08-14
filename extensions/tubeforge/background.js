// TubeForge extension — background service worker.
// Proxies API calls to the local TubeForge server (host_permissions bypasses
// CORS for extensions, so the loopback-only server stays unchanged and
// secure). Also answers liveness checks from the content script / popup.

const SERVER = "http://127.0.0.1:17487";

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg?.type === "tf:fetch") {
    fetch(`${SERVER}${msg.path}`)
      .then(async (r) => {
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        const text = await r.text();
        try {
          sendResponse({ ok: true, data: JSON.parse(text) });
        } catch {
          sendResponse({ ok: true, data: text });
        }
      })
      .catch((e) => sendResponse({ ok: false, error: String(e) }));
    return true; // async response
  }

  if (msg?.type === "tf:status") {
    fetch(`${SERVER}/api/healthz`)
      .then(async (r) => {
        const body = await r.text().catch(() => "");
        sendResponse({ ok: r.ok, body });
      })
      .catch(() => sendResponse({ ok: false, body: "" }));
    return true;
  }

  return false;
});
