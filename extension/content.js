// chanceify™: adds a one-click play button to songs on Last.fm (and YouTube).
// Nothing leaves your computer except the chanceify:// link that opens the app.
(() => {
  if (window.__cfyLoaded) return;
  window.__cfyLoaded = true;

  const ICON_PLAY = '<svg viewBox="0 0 24 24"><path d="M7 4.5v15l13-7.5z"/></svg>';
  const ICON_QUEUE = '<svg viewBox="0 0 24 24"><path d="M3 6h13v2H3zm0 5h13v2H3zm0 5h8v2H3zm14-1v-3h2v3h3v2h-3v3h-2v-3h-3v-2z"/></svg>';
  const ICON_WEB = '<svg viewBox="0 0 24 24"><path d="M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20zm6.9 9h-3.1a15.7 15.7 0 0 0-1.3-6A8 8 0 0 1 18.9 11zM12 4c.8 1 1.7 3 1.8 7h-3.6c.1-4 1-6 1.8-7zM9.5 5a15.7 15.7 0 0 0-1.3 6H5.1A8 8 0 0 1 9.5 5zM5.1 13h3.1c.1 2.3.6 4.4 1.3 6A8 8 0 0 1 5.1 13zm5.1 0h3.6c-.1 4-1 6-1.8 7-.8-1-1.7-3-1.8-7zm4.3 6c.7-1.6 1.2-3.7 1.3-6h3.1a8 8 0 0 1-4.4 6z"/></svg>';
  // chanceify's web page for a song or artist: it opens the song in chanceify
  // or looks it up on Spotify, Last.fm, YouTube and SoundCloud.
  const WORKER = 'https://chanceify-embed.chance-a10.workers.dev';
  const defaults = { replace: true, toast: true };
  let opts = { ...defaults };
  const api = typeof browser !== 'undefined' ? browser : typeof chrome !== 'undefined' ? chrome : null;

  const link = (mode, artist, track) =>
    mode === 'artist'
      ? `chanceify://artist?artist=${encodeURIComponent(artist)}`
      : `chanceify://${mode}?artist=${encodeURIComponent(artist)}&track=${encodeURIComponent(track)}`;

  let toastEl, toastTimer;
  function toast(text) {
    if (!opts.toast) return;
    if (!toastEl) {
      toastEl = document.createElement('div');
      toastEl.className = 'cfy-toast';
      document.body.appendChild(toastEl);
    }
    toastEl.textContent = text;
    requestAnimationFrame(() => toastEl.classList.add('cfy-on'));
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => toastEl.classList.remove('cfy-on'), 1800);
  }

  function send(mode, artist, track) {
    // A link click is the most reliable way to hand off to a desktop app.
    const a = document.createElement('a');
    a.href = link(mode, artist, track);
    a.style.display = 'none';
    document.body.appendChild(a);
    a.click();
    a.remove();
    toast(mode === 'queue' ? `added to queue: ${track}` : `playing in chanceify: ${mode === 'artist' ? artist : track}`);
  }

  function makeWebButton(artist, track, hero) {
    const b = document.createElement('a');
    b.className = 'cfy-btn cfy-web' + (hero ? ' cfy-hero' : '');
    b.href = `${WORKER}/find?a=${encodeURIComponent(artist)}` + (track ? `&t=${encodeURIComponent(track)}` : '');
    b.target = '_blank';
    b.rel = 'noopener';
    b.title = 'open chanceify\u2019s web page for this';
    b.setAttribute('aria-label', b.title);
    b.innerHTML = ICON_WEB + (hero ? '<span>web page</span>' : '');
    b.addEventListener('click', (e) => e.stopPropagation());
    return b;
  }

  function makeButton(mode, artist, track, hero) {
    const b = document.createElement('a');
    b.className = 'cfy-btn' + (mode === 'queue' ? ' cfy-queue' : '') + (hero ? ' cfy-hero' : '');
    b.href = link(mode, artist, track);
    b.setAttribute('role', 'button');
    b.title = mode === 'queue' ? 'add to chanceify queue' : 'play in chanceify';
    b.setAttribute('aria-label', b.title);
    b.innerHTML = (mode === 'queue' ? ICON_QUEUE : ICON_PLAY) + (hero ? '<span>play in chanceify</span>' : '');
    b.addEventListener('click', (e) => {
      e.preventDefault();
      e.stopPropagation();
      send(e.shiftKey ? 'queue' : mode, artist, track);
    });
    b.addEventListener('contextmenu', (e) => {
      if (hero) return;
      e.preventDefault();
      send('queue', artist, track);
    });
    return b;
  }

  const text = (el) => (el && el.textContent ? el.textContent.trim() : '');

  function decorateRows() {
    document.querySelectorAll('.chartlist-row:not([data-cfy])').forEach((row) => {
      // A row of artists (Top Artists): the link goes to /music/<artist> only.
      const nameLink = row.querySelector('.chartlist-name a');
      const path = nameLink ? (nameLink.getAttribute('href') || '').split('?')[0] : '';
      if (/^\/music\/[^/]+\/?$/.test(path) && !row.querySelector('.chartlist-artist')) {
        const artistName = text(nameLink);
        if (!artistName) return;
        row.setAttribute('data-cfy', '1');
        const b = makeButton('artist', artistName, '', false);
        b.title = 'play this artist in chanceify';
        b.setAttribute('aria-label', b.title);
        b.style.marginLeft = '10px';
        nameLink.parentNode.insertBefore(b, nameLink.nextSibling);
        const web = makeWebButton(artistName, '', false);
        web.style.marginLeft = '0';
        b.after(web);
        return;
      }
      const title = text(row.querySelector('.chartlist-name a, .chartlist-name'));
      let artist = text(row.querySelector('.chartlist-artist a, .chartlist-artist'));
      if (!artist) {
        // Album and artist pages list tracks without an artist column.
        artist = text(document.querySelector('.header-new-crumb[itemprop="url"], .header-new-title a, [data-artist-name]')) ||
          (document.querySelector('[data-artist-name]') || {}).dataset?.artistName || '';
      }
      if (!title || !artist) return;
      row.setAttribute('data-cfy', '1');
      const slot = row.querySelector('.chartlist-play') || row.querySelector('td');
      if (!slot) return;
      const b = makeButton('play', artist, title, false);
      const old = slot.querySelector('.chartlist-play-button, .chartlist-play-image');
      // The two buttons side by side, whatever width the cell gives them.
      const pair = document.createElement('span');
      pair.className = 'cfy-pair';
      pair.append(b, makeWebButton(artist, title, false));
      if (old && opts.replace) slot.insertBefore(pair, old);
      else slot.appendChild(pair);
    });
  }

  function decorateTrackPage() {
    if (document.querySelector('.cfy-hero')) return;
    const m = location.pathname.match(/^\/music\/([^/]+)\/_\/([^/?#]+)/);
    if (!m) return;
    const artist = decodeURIComponent(m[1].replace(/\+/g, ' '));
    const track = decodeURIComponent(m[2].replace(/\+/g, ' '));
    const b = makeButton('play', artist, track, true);
    const web = makeWebButton(artist, track, true);
    // Under Last.fm's own buttons (love, bookmark, more), in their style: a
    // row of its own that wraps below the buttons and the listener counts.
    const actions = document.querySelector('.header-new-actions');
    if (actions && actions.parentElement) {
      const row = document.createElement('div');
      row.className = 'cfy-hero-row';
      row.append(b, web);
      actions.parentElement.style.flexWrap = 'wrap';
      actions.parentElement.appendChild(row);
      return;
    }
    const host = document.querySelector('.header-new-content, .header-new-title');
    if (!host) return;
    host.parentNode.insertBefore(b, host.nextSibling);
    b.after(web);
  }

  // YouTube: an "open in chanceify" button under the video. The title is
  // cleaned into an artist and a song, and chanceify finds it on Spotify.
  function youtubeSong() {
    const raw = text(document.querySelector('ytd-watch-metadata h1, #title h1, h1.ytd-watch-metadata, ytmusic-player-bar .title'));
    if (!raw) return null;
    let channel = text(document.querySelector('ytd-watch-metadata ytd-channel-name a, #owner #channel-name a, ytmusic-player-bar .byline'));
    let title = raw
      .replace(/[\[(\u3010\uFF08][^\])\u3011\uFF09]*(official|lyric|audio|video|visuali[sz]er|hd|hq|4k|8k|remaster|music video|mv|prod\.?|explicit|clean|slowed|reverb|sped up|free|download|out now|premiere|full)[^\])\u3011\uFF09]*[\])\u3011\uFF09]/gi, '')
      .replace(/#\S+/g, '')
      .replace(/["\u201C\u201D\u300C\u300D]/g, '')
      .replace(/\s+(official|lyrics?|audio|music video)\s*$/i, '')
      .replace(/\s+/g, ' ')
      .trim();
    let artist = '';
    const split = title.split(/\s+[-\u2013\u2014|]\s+/);
    if (split.length >= 2) {
      artist = split[0].trim();
      title = split.slice(1).join(' - ').trim();
    } else {
      artist = channel.replace(/\s*-\s*Topic$/i, '').replace(/VEVO$/i, '').trim();
    }
    title = title.replace(/\s*(ft\.?|feat\.?)\s.*$/i, '').trim();
    return title ? { artist, title } : null;
  }

  function decorateYoutube() {
    if (!/(^|\.)youtube\.com$/.test(location.hostname)) return;
    const onWatch = location.pathname === '/watch' || location.hostname === 'music.youtube.com';
    const old = document.querySelector('.cfy-yt');
    if (!onWatch) { if (old) old.remove(); document.querySelectorAll('.cfy-ytweb').forEach((e) => e.remove()); return; }
    const song = youtubeSong();
    if (!song) return;
    const key = song.artist + '|' + song.title;
    if (old && old.dataset.cfy === key) return;
    if (old) old.remove();
    document.querySelectorAll('.cfy-ytweb').forEach((e) => e.remove());
    const host = document.querySelector('ytd-watch-metadata #top-level-buttons-computed, ytd-watch-metadata #actions-inner, #above-the-fold #title');
    if (!host) return;
    const b = makeButton('play', song.artist, song.title, true);
    b.classList.add('cfy-yt');
    b.dataset.cfy = key;
    b.querySelector('span').textContent = 'open in chanceify';
    host.appendChild(b);
    const web = makeWebButton(song.artist, song.title, true);
    web.classList.add('cfy-ytweb');
    b.after(web);
  }

  function apply() {
    document.documentElement.classList.toggle('cfy-hide-play', !!opts.replace);
    try { decorateRows(); decorateTrackPage(); decorateYoutube(); } catch (e) { /* never break the page */ }
  }

  let queued = false;
  new MutationObserver(() => {
    if (queued) return;
    queued = true;
    setTimeout(() => { queued = false; apply(); }, 250);
  }).observe(document.documentElement, { childList: true, subtree: true });

  if (api && api.storage && api.storage.sync) {
    api.storage.sync.get(defaults, (v) => { opts = { ...defaults, ...(v || {}) }; apply(); });
    api.storage.onChanged.addListener((c) => {
      for (const k in c) opts[k] = c[k].newValue;
      apply();
    });
  } else {
    apply();
  }
})();
