# chanceify™

*music reimagined .*

A small, fast Spotify player for your desktop. Needs Spotify Premium.

- Plays your Spotify, queue, playlists, lyrics and visualizers
- Music stats: top artists and songs for 7 days, 30 days or all time
- Last.fm scrobbling and missing cover art
- 16 themes, or pick your own colours
- Full screen visualizer with swirl, bass jump, lyrics and a swaying title
- Pop-out controls, columns you can lay out your way, mini player and calm mode
- Every key can be changed, exported and shared
- Everything it saves stays in one folder next to the app

## Get it

Download `chanceify.exe` from [Releases](https://github.com/madlygeeked/chanceify/releases),
put it in its own folder and run it.

More help: [madlygeeked.github.io/chanceify](https://madlygeeked.github.io/chanceify)

## Build it yourself

```
cargo build --release
```

Put your own Last.fm key and secret on two lines in `lastfm-keys.txt` first
(optional; the file is never committed).

---

Built with love by [chance](https://github.com/madlygeeked).
Inspired by [Spotify](https://www.spotify.com), [Spotifast](https://github.com/crmne/spotifast) and [Spicetify](https://spicetify.app). Licensed AGPL-3.0, see [LICENSE](LICENSE) and [NOTICE](NOTICE.md).
