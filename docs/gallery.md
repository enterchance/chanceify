---
title: gallery
permalink: /gallery/
---
<style>
.gal-sub{opacity:.75;margin:0 0 8px}
.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(230px,1fr));gap:14px;margin:12px 0 8px}
.grid figure{margin:0}
.grid img{display:block;width:100%;aspect-ratio:16/9;object-fit:cover;object-position:top;border-radius:10px;border:1px solid rgba(255,255,255,.14);cursor:zoom-in;background:rgba(10,9,20,.5);transition:transform .15s}
.grid img:hover{transform:scale(1.02)}
.grid figure.tall img{object-fit:contain}
.grid figcaption{font-size:.82rem;opacity:.7;margin-top:5px}
.soon{aspect-ratio:16/9;border:2px dashed rgba(255,255,255,.2);border-radius:10px;display:flex;flex-direction:column;align-items:center;justify-content:center;text-align:center;opacity:.6;padding:8px;font-size:.9rem}
.soon span{font-size:.72rem;opacity:.8}
.feature{margin-top:40px;padding:18px 20px;border-radius:12px;background:rgba(10,9,20,.6);border:1px solid rgba(255,255,255,.12)}
dialog.lb{border:0;background:transparent;padding:0;max-width:96vw;max-height:96vh}
dialog.lb::backdrop{background:rgba(0,0,0,.88)}
dialog.lb img{display:block;max-width:96vw;max-height:94vh;width:auto;height:auto;border-radius:8px;cursor:zoom-out}
</style>

<p class="gal-sub"><a href="{{ '/' | relative_url }}">back to chanceify</a>. tap a picture to make it bigger, tap again to close.</p>

# gallery

<h2>normal view</h2>
<div class="grid"><figure><a href="{{ '/gallery-images/normal-blue-lyrics.jpg' | relative_url }}" data-full><img src="{{ '/gallery-images/normal-blue-lyrics.jpg' | relative_url }}" alt="lyrics and queue beside the list" loading="lazy"></a><figcaption>lyrics and queue beside the list</figcaption></figure><figure><a href="{{ '/gallery-images/normal-green.jpg' | relative_url }}" data-full><img src="{{ '/gallery-images/normal-green.jpg' | relative_url }}" alt="green theme, swirl behind" loading="lazy"></a><figcaption>green theme, swirl behind</figcaption></figure><figure><a href="{{ '/gallery-images/popout-controls.jpg' | relative_url }}" data-full><img src="{{ '/gallery-images/popout-controls.jpg' | relative_url }}" alt="pop-out controls over the list" loading="lazy"></a><figcaption>pop-out controls over the list</figcaption></figure><figure><a href="{{ '/gallery-images/normal-red-lyrics.jpg' | relative_url }}" data-full><img src="{{ '/gallery-images/normal-red-lyrics.jpg' | relative_url }}" alt="red theme with lyrics" loading="lazy"></a><figcaption>red theme with lyrics</figcaption></figure><figure><a href="{{ '/gallery-images/normal-pink.jpg' | relative_url }}" data-full><img src="{{ '/gallery-images/normal-pink.jpg' | relative_url }}" alt="dark theme, pink cover" loading="lazy"></a><figcaption>dark theme, pink cover</figcaption></figure><figure><a href="{{ '/gallery-images/normal-tigerlily.jpg' | relative_url }}" data-full><img src="{{ '/gallery-images/normal-tigerlily.jpg' | relative_url }}" alt="playlist page" loading="lazy"></a><figcaption>playlist page</figcaption></figure><figure><a href="{{ '/gallery-images/normal-yump.jpg' | relative_url }}" data-full><img src="{{ '/gallery-images/normal-yump.jpg' | relative_url }}" alt="green theme, bars" loading="lazy"></a><figcaption>green theme, bars</figcaption></figure></div>
<h2>full screen visualizer</h2>
<div class="grid"><figure><a href="{{ '/gallery-images/full-vis-gray.jpg' | relative_url }}" data-full><img src="{{ '/gallery-images/full-vis-gray.jpg' | relative_url }}" alt="gray swirl, song card and controls" loading="lazy"></a><figcaption>gray swirl (small picture, retake)</figcaption></figure><figure><a href="{{ '/gallery-images/full-vis-orange.jpg' | relative_url }}" data-full><img src="{{ '/gallery-images/full-vis-orange.jpg' | relative_url }}" alt="orange swirl, song card and controls" loading="lazy"></a><figcaption>orange swirl, song card and controls</figcaption></figure><figure><a href="{{ '/gallery-images/full-vis-swans.jpg' | relative_url }}" data-full><img src="{{ '/gallery-images/full-vis-swans.jpg' | relative_url }}" alt="swirl with a quiet bar" loading="lazy"></a><figcaption>swirl with a quiet bar</figcaption></figure><figure><a href="{{ '/gallery-images/full-vis-dieyoung.jpg' | relative_url }}" data-full><img src="{{ '/gallery-images/full-vis-dieyoung.jpg' | relative_url }}" alt="scrolling cover art" loading="lazy"></a><figcaption>scrolling cover art</figcaption></figure><div class="soon"><span>coming soon</span>full screen lyrics</div><div class="soon"><span>coming soon</span>full screen lyrics with a visualizer behind</div></div>
<h2>calm mode</h2>
<div class="grid"><div class="soon"><span>coming soon</span>calm mode</div></div>
<h2>mini player</h2>
<div class="grid"><figure class="tall"><a href="{{ '/gallery-images/mini-player.jpg' | relative_url }}" data-full><img src="{{ '/gallery-images/mini-player.jpg' | relative_url }}" alt="mini player" loading="lazy"></a><figcaption>mini player</figcaption></figure><figure class="tall"><a href="{{ '/gallery-images/mini-player-queue.jpg' | relative_url }}" data-full><img src="{{ '/gallery-images/mini-player-queue.jpg' | relative_url }}" alt="mini player with the queue" loading="lazy"></a><figcaption>mini player with the queue</figcaption></figure></div>
<h2>extra windows</h2>
<div class="grid"><div class="soon"><span>coming soon</span>new visualizer window</div><div class="soon"><span>coming soon</span>detached controls</div><div class="soon"><span>coming soon</span>views panel</div></div>
<h2>visualizer settings</h2>
<div class="grid"><figure><a href="{{ '/gallery-images/vis-settings-bass.jpg' | relative_url }}" data-full><img src="{{ '/gallery-images/vis-settings-bass.jpg' | relative_url }}" alt="bass and colour" loading="lazy"></a><figcaption>bass and colour</figcaption></figure><figure><a href="{{ '/gallery-images/vis-settings-swirl.jpg' | relative_url }}" data-full><img src="{{ '/gallery-images/vis-settings-swirl.jpg' | relative_url }}" alt="swirl" loading="lazy"></a><figcaption>swirl</figcaption></figure><figure><a href="{{ '/gallery-images/vis-settings-flow.jpg' | relative_url }}" data-full><img src="{{ '/gallery-images/vis-settings-flow.jpg' | relative_url }}" alt="flow, with the song right-click menu" loading="lazy"></a><figcaption>flow, with the song right-click menu</figcaption></figure><div class="soon"><span>coming soon</span>equalizer</div></div>
<h2>song lists</h2>
<div class="grid"><div class="soon"><span>coming soon</span>column heading menu</div><div class="soon"><span>coming soon</span>playlists column grids</div><div class="soon"><span>coming soon</span>sorted by playlists</div><div class="soon"><span>coming soon</span>liked songs</div><div class="soon"><span>coming soon</span>album page</div><div class="soon"><span>coming soon</span>artist page</div><div class="soon"><span>coming soon</span>search</div><div class="soon"><span>coming soon</span>queue</div><div class="soon"><span>coming soon</span>home</div></div>
<h2>player</h2>
<div class="grid"><div class="soon"><span>coming soon</span>volume presets menu</div><div class="soon"><span>coming soon</span>back button history</div><div class="soon"><span>coming soon</span>unavailable songs</div></div>
<h2>settings</h2>
<div class="grid"><figure><a href="{{ '/gallery-images/settings-playback.jpg' | relative_url }}" data-full><img src="{{ '/gallery-images/settings-playback.jpg' | relative_url }}" alt="playback settings" loading="lazy"></a><figcaption>playback settings</figcaption></figure><div class="soon"><span>coming soon</span>look and themes</div><div class="soon"><span>coming soon</span>accounts</div><div class="soon"><span>coming soon</span>library</div><div class="soon"><span>coming soon</span>network</div><div class="soon"><span>coming soon</span>about</div><div class="soon"><span>coming soon</span>last.fm</div><div class="soon"><span>coming soon</span>keybinds</div><div class="soon"><span>coming soon</span>first-run tour</div></div>
<h2>discord</h2>
<div class="grid"><div class="soon"><span>coming soon</span>discord presence</div></div>
<h2>wallpaper engine</h2>
<div class="grid"><figure><a href="{{ '/gallery-images/wallpaper-engine.jpg' | relative_url }}" data-full><img src="{{ '/gallery-images/wallpaper-engine.jpg' | relative_url }}" alt="chanceify at 1080p on an ultrawide wallpaper" loading="lazy"></a><figcaption>chanceify at 1080p on an ultrawide wallpaper</figcaption></figure></div><p class="plug">chanceify as a live wallpaper. link coming soon.</p>

<div class="feature"><b>want to be featured?</b> show off how yours looks. <a href="https://github.com/madlygeeked/chanceify/issues/new?template=get-featured.yml">submit your screenshot</a> (a free github account is needed). good ones go up here.</div>

<dialog class="lb" id="lb"><img id="lbimg" alt=""></dialog>
<script>
(function(){var d=document.getElementById('lb'),i=document.getElementById('lbimg');
document.querySelectorAll('[data-full]').forEach(function(a){a.addEventListener('click',function(e){e.preventDefault();i.src=a.href;i.alt=a.querySelector('img').alt;d.showModal();});});
d.addEventListener('click',function(){d.close();});})();
</script>
