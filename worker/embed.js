// chanceify™ song page: a Cloudflare Worker that makes the link "Copy the song
// for Discord" gives out. Discord, and anyone who opens it, sees the cover,
// the song, and a button that opens it in Spotify. No keys, no secrets.
//
//   https://<your-worker>.workers.dev/t/<spotify track id>?t=<title>&a=<artist>
//
// The cover comes from Spotify's public oEmbed. The title and artist come from
// the link itself (chanceify puts them there), with oEmbed as the fallback.

const esc = (s) =>
  String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

export default {
  async fetch(request) {
    const url = new URL(request.url);
    const match = url.pathname.match(/^\/t\/([A-Za-z0-9]{10,30})\/?$/);
    if (!match) {
      return new Response(page({ title: "chanceify™", artist: "A Spotify player by chance", id: null }), html());
    }
    const id = match[1];
    let title = (url.searchParams.get("t") || "").slice(0, 150);
    const artist = (url.searchParams.get("a") || "").slice(0, 150);
    let image = "";
    try {
      const res = await fetch(
        "https://open.spotify.com/oembed?url=" + encodeURIComponent("https://open.spotify.com/track/" + id),
        { cf: { cacheTtl: 3600, cacheEverything: true } },
      );
      if (res.ok) {
        const data = await res.json();
        image = data.thumbnail_url || "";
        if (!title) title = data.title || "";
      }
    } catch (_) {}
    return new Response(page({ title: title || "A song", artist, id, image }), html());
  },
};

const html = () => ({ headers: { "content-type": "text/html; charset=utf-8", "cache-control": "public, max-age=600" } });

function page({ title, artist, id, image }) {
  const spotify = id ? `https://open.spotify.com/track/${id}` : "https://github.com/madlygeeked/chanceify";
  const line = artist ? `${artist}` : "Listening on chanceify™";
  const desc = artist ? `${artist}. Listening on chanceify™` : "Listening on chanceify™";
  return `<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>${esc(title)} - chanceify™</title>
<meta name="theme-color" content="#8b5cf6">
<meta property="og:type" content="music.song">
<meta property="og:site_name" content="chanceify™">
<meta property="og:title" content="${esc(title)}">
<meta property="og:description" content="${esc(desc)}">
${image ? `<meta property="og:image" content="${esc(image)}">` : ""}
<meta property="og:url" content="${esc(spotify)}">
<meta name="twitter:card" content="summary_large_image">
${image ? `<meta name="twitter:image" content="${esc(image)}">
<meta property="og:image:width" content="640">
<meta property="og:image:height" content="640">` : ""}
<style>
:root{color-scheme:dark}
*{box-sizing:border-box}
body{margin:0;min-height:100vh;display:grid;place-items:center;padding:24px;font-family:system-ui,"Segoe UI",sans-serif;color:#f3f0ff;
background:radial-gradient(900px 600px at 20% 10%,#4c1d95 0,transparent 60%),radial-gradient(800px 600px at 90% 90%,#312e81 0,transparent 60%),#0f0b1d}
.card{width:min(420px,100%);text-align:center}
.cover{width:100%;aspect-ratio:1;border-radius:22px;background:#241a45 center/cover;box-shadow:0 30px 80px #0009,0 0 0 1px #ffffff1a}
h1{font-size:26px;margin:22px 0 4px;line-height:1.2}
p{margin:0;color:#c4b5fd;font-size:16px}
.btn{display:inline-block;margin-top:26px;padding:13px 28px;border-radius:999px;background:linear-gradient(90deg,#6366f1,#8b5cf6);color:#fff;font-weight:600;text-decoration:none}
.btn:hover{filter:brightness(1.12)}
.row{margin-top:14px;display:flex;gap:10px;justify-content:center;flex-wrap:wrap}
.ghost{padding:9px 16px;border-radius:999px;border:1px solid #a78bfa55;color:#ddd6fe;text-decoration:none;font-size:14px}
.ghost:hover{background:#a78bfa22}
.foot{margin-top:28px;font-size:13px;color:#a78bfa99}
.foot a{color:inherit}
</style></head><body>
<main class="card">
<div class="cover"${image ? ` style="background-image:url('${esc(image)}')"` : ""}></div>
<h1>${esc(title)}</h1>
<p>${esc(line)}</p>
${id ? `<a class="btn" href="${esc(spotify)}">Listen on Spotify</a>
<div class="row"><a class="ghost" href="spotify:track:${esc(id)}">Open in the Spotify app</a><a class="ghost" href="chanceify://track/${esc(id)}">Open in chanceify</a></div>` : ""}
<div class="foot">Built with love by <a href="https://github.com/madlygeeked">chance</a></div>
</main></body></html>`;
}
