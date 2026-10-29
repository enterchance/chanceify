---
title: chanceify™
hero: true
tagline: music reimagined
---

a spotify player for windows.  
needs spotify premium to stream.  
local library works without it.

## get it

1. download `chanceify.exe` from [releases](https://github.com/enterchance/chanceify/releases)
2. run it. [virustotal](https://www.virustotal.com/gui/file/69fea436120c0f9e6be32e125f2fdeec441a8b1c47d15d533e255e88557319c0)

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
- **right-click anything** to bring up settings.

{% assign anyshots = site.static_files | where_exp: "f", "f.path contains '/shots/'" %}
{% if anyshots.size > 0 %}
{% include gallery.html items=site.data.shots.panels %}
{% endif %}

## setup

1. open the [spotify developer dashboard](https://developer.spotify.com/dashboard) and sign in.
2. create an app. any name.
3. redirect uri: `http://127.0.0.1:8989/login`
4. tick web api, save, copy the client id.
5. in chanceify: settings, personal spotify app. paste it and authorize.

**last.fm:** settings, last.fm, connect. allow access in your browser. your plays scrobble.

## questions

**do i need premium?** only to stream, blame spotify.

**where is everything saved?** in the folder beside `chanceify.exe`.

**does it download songs?** nope. it streams using [librespot](https://github.com/librespot-org/librespot)

**does it collect data?** nope. [privacy and terms](https://github.com/enterchance/chanceify/blob/main/PRIVACY.md)

**something broke?** [issues](https://github.com/enterchance/chanceify/issues)
