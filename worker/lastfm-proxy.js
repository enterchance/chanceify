// chanceify™ Last.fm proxy: a Cloudflare Worker that keeps the Last.fm API
// key and shared secret out of the app and out of GitHub.
//
// Secrets to add in Cloudflare (Settings > Variables and Secrets, type "Secret"):
//   LASTFM_KEY      your Last.fm API key
//   LASTFM_SECRET   your Last.fm shared secret
//
// The app sends the request without a key or signature. This worker adds the
// key, signs it with the secret, forwards it to Last.fm and returns the answer.

const API = "https://ws.audioscrobbler.com/2.0/";

// Only what chanceify needs. Anything else is refused.
const SIGNED = new Set([
  "auth.getToken",
  "auth.getSession",
  "track.scrobble",
  "track.updateNowPlaying",
  "track.love",
  "track.unlove",
  "user.getInfo",
]);
const PLAIN = new Set(["album.getinfo", "artist.getinfo"]);

async function md5(text) {
  const bytes = new TextEncoder().encode(text);
  const hash = await crypto.subtle.digest("MD5", bytes);
  return [...new Uint8Array(hash)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

async function sign(params, secret) {
  const names = Object.keys(params).filter((n) => n !== "format" && n !== "callback").sort();
  let text = "";
  for (const n of names) text += n + params[n];
  return md5(text + secret);
}

const json = (body, status = 200) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json", "access-control-allow-origin": "*" },
  });

export default {
  async fetch(request, env) {
    if (request.method === "OPTIONS") return json({});
    let params = {};
    if (request.method === "POST") {
      const form = await request.formData().catch(() => null);
      if (!form) return json({ error: 6, message: "bad request" }, 400);
      for (const [k, v] of form.entries()) params[k] = String(v);
    } else if (request.method === "GET") {
      for (const [k, v] of new URL(request.url).searchParams.entries()) params[k] = v;
    } else {
      return json({ error: 6, message: "bad method" }, 405);
    }
    delete params.api_key;
    delete params.api_sig;
    const method = params.method || "";
    params.format = "json";
    params.api_key = env.LASTFM_KEY;

    let signed = SIGNED.has(method);
    if (!signed && !PLAIN.has(method.toLowerCase())) {
      return json({ error: 6, message: "method not allowed" }, 403);
    }
    if (signed) params.api_sig = await sign(params, env.LASTFM_SECRET);

    const body = new URLSearchParams(params);
    const reply = await fetch(API, {
      method: "POST",
      headers: { "content-type": "application/x-www-form-urlencoded", "user-agent": "chanceify-proxy" },
      body,
    });
    const data = await reply.json().catch(() => ({ error: 16, message: "unreadable answer" }));
    // The sign-in page needs the public key. It is public by nature; the secret never leaves.
    if (method === "auth.getToken" && !data.error) data.api_key = env.LASTFM_KEY;
    return json(data, reply.status);
  },
};
