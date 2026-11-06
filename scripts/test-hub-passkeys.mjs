// End-to-end check of Portal Hub passkey pages in a real Chromium browser with
// a virtual WebAuthn authenticator: owner setup link, sign-in and recovery.
// No npm packages: Node 22+'s WebSocket and the DevTools protocol.
//
//   cargo build -p portal-hub
//   node scripts/test-hub-passkeys.mjs target/debug/portal-hub
//
// Set CHROME to the browser executable (default: chromium). Runs only local
// processes (Hub, browser) and removes their temporary directories.
import { spawn, execFileSync } from "node:child_process";
import { createServer } from "node:http";
import { createHash, randomBytes } from "node:crypto";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const HUB = process.argv[2] || "target/debug/portal-hub";
const CHROME = process.env.CHROME || "chromium";
const port = 18000 + Math.floor(Math.random() * 1000);
const origin = `http://portal-hub.localhost:${port}`;
const state = mkdtempSync(join(tmpdir(), "hub-e2e-state-"));
const profile = mkdtempSync(join(tmpdir(), "hub-e2e-chrome-"));
const env = { ...process.env, PORTAL_HUB_RUN_DIR: join(state, "run"), PORTAL_HUB_REQUIRE_BOOTSTRAP: "true", PORTAL_HUB_PUBLIC_URL: origin };
delete env.SSH_ORIGINAL_COMMAND;
const children = [];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const check = (ok, message) => { if (!ok) throw new Error("FAILED: " + message); console.log("ok -", message); };
const b64url = (buf) => buf.toString("base64url");

function hubCli(...args) {
  return JSON.parse(execFileSync(HUB, ["--state-dir", join(state, "data"), ...args], { env }).toString());
}

// Loopback OAuth callback, like Portal desktop's listener.
const callbacks = [];
const callbackServer = createServer((req, res) => {
  const url = new URL(req.url, "http://127.0.0.1");
  if (url.pathname === "/callback") callbacks.push(url);
  res.end("<h1>Portal Hub sign-in complete</h1>");
});
await new Promise((r) => callbackServer.listen(0, "127.0.0.1", r));
const redirectUri = `http://127.0.0.1:${callbackServer.address().port}/callback`;

function oauth() {
  const verifier = b64url(randomBytes(32));
  const params = new URLSearchParams({
    response_type: "code", client_id: "portal-desktop", redirect_uri: redirectUri,
    code_challenge: b64url(createHash("sha256").update(verifier).digest()),
    code_challenge_method: "S256", state: b64url(randomBytes(24)),
  });
  return { verifier, params };
}

async function exchange(code, verifier) {
  const response = await fetch(`http://127.0.0.1:${port}/oauth/token`, {
    method: "POST", headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({ grant_type: "authorization_code", code, redirect_uri: redirectUri, client_id: "portal-desktop", code_verifier: verifier }),
  });
  check(response.ok, "authorization code exchanges for tokens");
  const tokens = await response.json();
  const me = await (await fetch(`http://127.0.0.1:${port}/api/me`, { headers: { authorization: "Bearer " + tokens.access_token } })).json();
  return me;
}

async function waitForCallback(count, ms = 15000) {
  const deadline = Date.now() + ms;
  while (callbacks.length < count && Date.now() < deadline) await sleep(100);
  check(callbacks.length >= count, `browser returned to Portal's loopback callback (#${count})`);
  return callbacks[count - 1];
}

try {
  const hub = spawn(HUB, ["--state-dir", join(state, "data"), "web", "--bind", `127.0.0.1:${port}`, "--public-url", origin], { env, stdio: ["ignore", "inherit", "inherit"] });
  children.push(hub);
  for (let i = 0; i < 100; i++) {
    try { if ((await fetch(`http://127.0.0.1:${port}/api/info`)).ok) break; } catch {}
    await sleep(100);
  }

  const chrome = spawn(CHROME, ["--headless=new", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "--no-first-run", "--no-default-browser-check", "about:blank"], { stdio: ["ignore", "ignore", "pipe"] });
  children.push(chrome);
  const wsUrl = await new Promise((resolve, reject) => {
    let buffer = "";
    chrome.stderr.on("data", (d) => { buffer += d; const m = buffer.match(/DevTools listening on (ws:\/\/\S+)/); if (m) resolve(m[1]); });
    setTimeout(() => reject(new Error("chromium did not start")), 20000);
  });
  const ws = new WebSocket(wsUrl);
  await new Promise((r) => ws.addEventListener("open", r, { once: true }));
  let nextId = 1;
  const pending = new Map();
  ws.addEventListener("message", (event) => {
    const msg = JSON.parse(event.data);
    if (msg.id && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      pending.delete(msg.id);
      msg.error ? reject(new Error(JSON.stringify(msg.error))) : resolve(msg.result);
    }
  });
  const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, { resolve, reject });
    ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
  });
  const { targetId } = await send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
  const page = (method, params) => send(method, params, sessionId);
  await page("Page.enable");
  await page("WebAuthn.enable", { enableUI: false });
  const addAuthenticator = async () => (await page("WebAuthn.addVirtualAuthenticator", { options: {
    protocol: "ctap2", transport: "internal", hasResidentKey: true, hasUserVerification: true,
    isUserVerified: true, automaticPresenceSimulation: true } })).authenticatorId;
  const evaluate = async (expression) => (await page("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true })).result.value;
  const navigate = async (url) => {
    await page("Page.navigate", { url });
    for (let i = 0; i < 100; i++) {
      await sleep(100);
      try { if (await evaluate("document.readyState") === "complete") return; } catch {}
    }
  };
  const waitFor = async (expression, label) => {
    for (let i = 0; i < 100; i++) {
      if (await evaluate(expression)) return;
      await sleep(100);
    }
    throw new Error("timed out waiting for " + label + ": " + await evaluate("document.body.innerText"));
  };

  // 1. A new Hub sends visitors to Portal; sign-in is not offered yet.
  await navigate(`${origin}/oauth/authorize?${oauth().params}`);
  check((await evaluate("document.body.innerText")).includes("Finish setup in Portal"), "new Hub asks to finish setup in Portal");
  check(!(await evaluate("!!document.querySelector('input[type=password]')")), "no password field anywhere");

  // 2. Portal's setup link creates the owner passkey and signs Portal in.
  let authenticator = await addAuthenticator();
  const setup = hubCli("owner-setup", "--json", "--username", "e2e-owner");
  check(setup.setup_url === `${origin}/setup#token=${setup.token}`, "CLI prints the setup link for the HTTPS-capable address");
  const first = oauth();
  await navigate(`${origin}/setup?${first.params}#token=${setup.token}`);
  await waitFor("!document.getElementById('setup-form').hidden", "setup form");
  check(!(await evaluate("location.href")).includes(setup.token), "setup token is removed from the address bar and history");
  check(await evaluate("document.getElementById('setup-account').textContent") === "e2e-owner", "setup page shows the account name");
  await evaluate("document.getElementById('setup-button').click()");
  let callback = await waitForCallback(1);
  check(callback.searchParams.get("state") === first.params.get("state"), "callback carries Portal's OAuth state");
  check((await exchange(callback.searchParams.get("code"), first.verifier)).username === "e2e-owner", "Portal is signed in as the new owner");
  check((await (await fetch(`http://127.0.0.1:${port}/webauthn/setup/info`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ token: setup.token }) })).status) === 400, "setup link cannot be used twice");
  check(hubCli("bootstrap-status", "--json").owner_ready === true, "Hub reports the owner as ready");

  // 3. Later sign-ins use the same passkey; the page starts the prompt itself.
  const second = oauth();
  await navigate(`${origin}/oauth/authorize?${second.params}`);
  callback = await waitForCallback(2);
  check((await exchange(callback.searchParams.get("code"), second.verifier)).username === "e2e-owner", "passkey sign-in works without typing anything");

  // 4. Recovery over SSH: a reset link replaces the passkey on a new device.
  const reset = hubCli("owner-setup", "--json", "--mode", "reset");
  await page("WebAuthn.removeVirtualAuthenticator", { authenticatorId: authenticator });
  authenticator = await addAuthenticator();
  const third = oauth();
  await navigate(`${origin}/setup?${third.params}#token=${reset.token}`);
  await waitFor("!document.getElementById('setup-form').hidden", "reset form");
  check((await evaluate("document.getElementById('setup-title').textContent")).includes("Replace"), "reset page explains the replacement");
  await evaluate("document.getElementById('setup-button').click()");
  callback = await waitForCallback(3);
  check((await exchange(callback.searchParams.get("code"), third.verifier)).username === "e2e-owner", "reset signs Portal in with the new passkey");
  const fourth = oauth();
  await navigate(`${origin}/oauth/authorize?${fourth.params}`);
  callback = await waitForCallback(4);
  check((await exchange(callback.searchParams.get("code"), fourth.verifier)).username === "e2e-owner", "new passkey signs in after reset");
  console.log("PASS: passkey setup, sign-in and recovery in Chromium");
  ws.close();
} finally {
  for (const child of children) child.kill("SIGTERM");
  callbackServer.close();
  await sleep(500);
  rmSync(state, { recursive: true, force: true });
  rmSync(profile, { recursive: true, force: true });
}
