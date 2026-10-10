# Last.fm proxy (Cloudflare Worker)

Keeps the Last.fm key and secret out of the app and GitHub.

1. Cloudflare dashboard, Workers & Pages, Create, "Hello World", name it `chanceify-lastfm`, Deploy.
2. Edit code: paste all of `lastfm-proxy.js`, Deploy.
3. Settings, Variables and Secrets, add two **Secrets**: `LASTFM_KEY` and `LASTFM_SECRET`.
4. Copy the worker address (https://chanceify-lastfm.<you>.workers.dev) and give it to the developer:
   it goes into `PROXY_URL` in `src/lastfm.rs`.

# Song link page (embed.js)

Makes the link "Copy the song for Discord" gives out: a pretty page with the cover, and Discord shows it as a card.

1. Cloudflare dashboard, Workers & Pages, Create, "Hello World", name it `chanceify-embed`, Deploy.
2. Edit code: paste all of `embed.js`, Deploy.
3. No secrets needed. Copy the address (https://chanceify-embed.<you>.workers.dev) and put it in `EMBED_URL` in `src/discord.rs`
   (until then the app copies the normal Spotify link).
