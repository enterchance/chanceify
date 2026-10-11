---
title: chanceify™
hero: true
tagline: music reimagined
---

needs spotify premium.

## get it

1. download `chanceify.exe` from [releases](https://github.com/madlygeeked/chanceify/releases)
2. run it. [virustotal](https://www.virustotal.com)

## features

- **plays your spotify.** no, really.
- **music stats.** top artists and songs, plus recommended.
- **last.fm.** scrobbling, love, and missing cover art saved for you.
- **discord.** shows what you're playing.
- **lyrics.** the words to the song.
- **looks.** themes, make your own, or let it follow the album cover.
- **visualizer.** bars, flow and swirl, full screen, in its own window or behind the lyrics.
- **views.** click the disc to switch parts of the screen on or off.
- **keybinds.** every key can be changed.
- **right-click anything** to bring up settings, and hover any button to see what it does.
- **gallery.** [pictures of everything]({{ '/gallery/' | relative_url }}).

{% assign anyshots = site.static_files | where_exp: "f", "f.path contains '/shots/'" %}
{% if anyshots.size > 0 %}
{% include gallery.html items=site.data.shots.panels %}
{% endif %}

## setup

**getting rate limited?** (optional) get more api requests with your own spotify app.

1. open the [spotify developer dashboard](https://developer.spotify.com/dashboard) and sign in.
2. create an app. any name.
3. redirect uri: `http://127.0.0.1:8989/login`
4. tick web api, save, copy the client id.
5. in chanceify: settings, personal spotify app. paste it and authorize.

**last.fm:** settings, last.fm, connect. allow access in your browser. your plays scrobble.

## questions

**do i need premium?** yes. blame spotify.

**where is everything saved?** in the folder beside `chanceify.exe`.

**does it download songs?** nope. it streams using [librespot](https://github.com/librespot-org/librespot)

**does it collect data?** nope. [privacy and terms](https://github.com/madlygeeked/chanceify/blob/main/PRIVACY.md)

**something broke?** [issues](https://github.com/madlygeeked/chanceify/issues)
