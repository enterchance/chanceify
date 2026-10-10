---
title: chanceify™
hero: true
tagline: music reimagined
---

needs spotify premium.

## get it

1. download `chanceify.exe` from [releases](https://github.com/madlygeeked/chanceify/releases).
2. run it. [virustotal.](https://www.virustotal.com)

## what it does

- listen to music from your spotify.
- scrobble to last.fm.
- customize it and set your own keybinds.

{% assign anyshots = site.static_files | where_exp: "f", "f.path contains '/shots/'" %}
{% if anyshots.size > 0 %}
## every panel

{% include gallery.html items=site.data.shots.panels %}

## every setting

{% include gallery.html items=site.data.shots.settings %}

## every theme

{% include gallery.html items=site.data.shots.themes %}
{% endif %}

## more

[all features](features) · [setup](setup) · [settings](settings) · [keys](keys) · [files](files) · [questions](faq)
