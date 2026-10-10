# How to bug test chanceify
Do one pass per build. Write down anything odd with the version number (Settings > About).

1. Fresh start: close chanceify, delete nothing, start it. It opens, no freeze.
2. Every view: press Z (or click the disc), then the five icons: Mini player (its own window), Normal, Full screen visualizer, Full screen lyrics, Calm mode. Also New visualizer window (visualizer or lyrics, wave and X buttons, drag, resize). Go through each twice, with the pop-out controls on and off.
3. Every key: Settings > Keys, press each key once. Nothing should crash.
4. Playback: play, pause, next, previous, seek, volume, shuffle, repeat, queue add, like and unlike.
5. Search: click the search box once, type, play a result. Also with a long name and a song with symbols.
6. Resize and zoom: drag the window small and large, change zoom, snap to a second monitor.
7. Themes: flip through all themes with see-through on and off.
8. Discord and Last.fm: song shows on Discord, scrobbles on Last.fm, nothing breaks when Discord is closed.
9. Long run: leave it playing for 2 hours, then check memory in Task Manager (should stay flat) and that it still responds.
10. Offline: turn Wi-Fi off for a minute, then on. It should recover without restarting.
11. Before 1.0: install on a clean Windows 10 virtual machine and repeat 1 to 5.
Also try tiny odd things: double-click everything, right-click everything, press Esc everywhere.
