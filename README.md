<p align="center">
  <img src="https://madlygeeked.github.io/assets/chanceify.png" alt="" width="120">
</p>

<h1 align="center">chanceify™</h1>
<p align="center"><i>music reimagined</i></p>

<p align="center">
  <a href="https://github.com/madlygeeked/chanceify/releases">download</a> &nbsp;·&nbsp;
  <a href="https://madlygeeked.github.io/chanceify/">more info</a> &nbsp;·&nbsp;
  <a href="https://github.com/madlygeeked/chanceify/issues">issues</a>
</p>

a spotify player for windows. needs spotify premium.

## get it

1. download `chanceify.exe` from [releases](https://github.com/madlygeeked/chanceify/releases)
2. run it, in its own folder. everything it saves stays beside it.

## features

- **plays your spotify.** no, really.
- **music stats.** top artists and songs, plus recommended.
- **last.fm.** scrobbling, love, and missing cover art saved for you.
- **lyrics.** a floating card that follows the song.
- **looks.** themes, make your own, or let it follow the album cover.
- **visualizer.** bars, flow and swirl, full screen, in its own window or behind the lyrics.
- **views.** click the disc to switch parts of the screen on or off.
- **keybinds.** every key can be changed.
- **discord.** shows what you're playing.

it streams with [librespot](https://github.com/librespot-org/librespot). it doesn't download songs, and it doesn't collect data ([privacy and terms](PRIVACY.md)).

more help: [chanceify](https://madlygeeked.github.io/chanceify/)

---

<details>
<summary>build it yourself</summary>

this is for windows. it takes about 15 minutes, most of it waiting.

**1. install the tools (once)**

- [rust](https://rustup.rs): download and run `rustup-init.exe`, press enter to accept the defaults.
- [build tools for visual studio](https://visualstudio.microsoft.com/downloads/): scroll to "tools for visual studio", install "build tools", and tick **desktop development with c++**. rust needs it.
- restart your computer (or at least close and reopen any terminal).

**2. get the code**

on the [github page](https://github.com/madlygeeked/chanceify), click **code**, then **download zip**. unzip it somewhere simple, like `C:\chanceify-source`. the folder you want is the one with `Cargo.toml` in it.

**3. last.fm (optional)**

the code ships with no last.fm key and no helper address, so you set up your own. get a key at [last.fm/api/account/create](https://www.last.fm/api/account/create): you get an api key and a shared secret. never share the secret, post it or commit it. then pick one:

- *easy:* make a file called `lastfm-keys.txt` in the folder with `Cargo.toml`. put the key on the first line and the secret on the second, nothing else. the keys are built into your own copy, and git never uploads the file. do not share a build made this way.
- *safer, if you will share your build:* keep the secret out of the app with a free [cloudflare worker](https://dash.cloudflare.com). open [`worker/README.md`](worker/README.md) and follow it (about 5 minutes: make a worker, paste in `worker/lastfm-proxy.js`, add two secrets called `LASTFM_KEY` and `LASTFM_SECRET`). then make a file called `lastfm-proxy.txt` in the folder with `Cargo.toml`, put your worker's address in it (one line), and build.

if you do neither, chanceify still works, and asks for a last.fm key in settings if you want scrobbling.

**4. build it**

open the folder with `Cargo.toml` in it, click the address bar at the top of the window, type `cmd` and press enter. a black window opens in that folder. type:

```
cargo build --release
```

the first build downloads and compiles a lot, so give it a while (10 minutes is normal). it fetches the right rust version by itself.

**5. run it**

your app is `target\release\chanceify.exe`. copy it into a folder of its own, like `C:\chanceify`, and run it from there. everything it saves stays in that folder.

</details>

---

<p align="center">
  built with love by <a href="https://github.com/madlygeeked">chance</a> <img src="https://madlygeeked.github.io/assets/favicon.png" alt="" width="20"><br>
  inspired by <a href="https://www.spotify.com">Spotify</a>, <a href="https://spotifast.rocks/">Spotifast</a> and <a href="https://spicetify.app">Spicetify</a><br>
  licensed AGPL-3.0, see <a href="LICENSE">LICENSE</a> and <a href="NOTICE.md">NOTICE</a>
</p>
