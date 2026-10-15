# chanceify™

Adds a purple play button to songs on Last.fm, and an "open in chanceify" button under YouTube videos. One click plays the song in chanceify; shift-click or right-click adds it to the queue.

- Firefox/Chrome: `python3 build.py`, then load `dist/chanceify-lastfm-<version>.zip` (temporary add-on via about:debugging, or sign it on addons.mozilla.org).
- Tampermonkey: install `dist/chanceify-lastfm.user.js`.
- Needs the chanceify Settings button "Let chanceify:// links open chanceify (Windows)" pressed once.

No data is collected. The only thing sent anywhere is the chanceify:// link, to your own computer.
