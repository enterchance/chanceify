#!/usr/bin/env python3
"""Builds chanceify-lastfm.zip (Firefox/Chrome) and chanceify-lastfm.user.js (Tampermonkey)."""
import json, pathlib, zipfile
here = pathlib.Path(__file__).parent
out = here / "dist"; out.mkdir(exist_ok=True)
ver = json.loads((here / "manifest.json").read_text())["version"]
files = ["manifest.json", "content.js", "content.css", "popup.html", "popup.css", "popup.js"] + [f"icons/{s}.png" for s in (16, 32, 48, 96, 128)]
with zipfile.ZipFile(out / f"chanceify-lastfm-{ver}.zip", "w", zipfile.ZIP_DEFLATED) as z:
    for f in files:
        z.write(here / f, f)
css = (here / "content.css").read_text()
js = (here / "content.js").read_text()
header = f"""// ==UserScript==
// @name         chanceify for Last.fm
// @namespace    https://madlygeeked.github.io/chanceify/
// @version      {ver}
// @description  Play any song on Last.fm in chanceify with one click.
// @match        https://www.last.fm/*
// @match        https://www.youtube.com/*
// @match        https://music.youtube.com/*
// @grant        none
// @run-at       document-idle
// ==/UserScript==
"""
inject = "(()=>{const s=document.createElement('style');s.textContent=" + json.dumps(css) + ";document.head.appendChild(s);})();\n"
(out / "chanceify-lastfm.user.js").write_text(header + inject + js)
print("built", ver)
