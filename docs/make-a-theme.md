---
title: make a theme
permalink: /make-a-theme/
noindex: true
---

<p><a href="{{ '/' | relative_url }}">back to chanceify</a></p>

# make a theme

a theme is a small text file.

1. settings, look, open themes folder.
2. make a new file there ending in `.json`, any name. it shows up in the theme list.
3. put this in it, and change the colours:

```json
{
  "base": "dark",
  "colors": {
    "window": "#2e3440",
    "panel": "#333945",
    "text": "#d8dee9",
    "accent": "#81a1c1"
  }
}
```

`base` is `dark` or `light`. any colour you leave out keeps the base one. colours are `#RRGGBB`. a typo in a name makes the file invalid, so check the spelling.

the colour names: `window`, `panel`, `surface`, `surface_hover`, `surface_active`, `outline`, `text`, `secondary`, `dim`, `accent`, `accent_hover`, `on_accent`, `danger`, `warning`, `overlay`, `shadow`.
