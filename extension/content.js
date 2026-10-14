// chanceify™: adds a one-click play button to songs on Last.fm (and YouTube).
// Nothing leaves your computer except the chanceify:// link that opens the app.
(() => {
  if (window.__cfyLoaded) return;
  window.__cfyLoaded = true;

  const ICON_PLAY = '<svg viewBox="0 0 24 24"><path d="M7 4.5v15l13-7.5z"/></svg>';
  const ICON_QUEUE = '<svg viewBox="0 0 24 24"><path d="M3 6h13v2H3zm0 5h13v2H3zm0 5h8v2H3zm14-1v-3h2v3h3v2h-3v3h-2v-3h-3v-2z"/></svg>';
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
      if (old && opts.replace) slot.insertBefore(b, old);
      else slot.appendChild(b);
    });
  }

  function decorateTrackPage() {
    if (document.querySelector('.cfy-hero')) return;
    const m = location.pathname.match(/^\/music\/([^/]+)\/_\/([^/?#]+)/);
    if (!m) return;
    const artist = decodeURIComponent(m[1].replace(/\+/g, ' '));
    const track = decodeURIComponent(m[2].replace(/\+/g, ' '));
    const host = document.querySelector('.header-new-actions, .header-new-content, .header-new-title');
    if (!host) return;
    const b = makeButton('play', artist, track, true);
    host.parentNode.insertBefore(b, host.nextSibling);
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
    if (!onWatch) { if (old) old.remove(); return; }
    const song = youtubeSong();
    if (!song) return;
    const key = song.artist + '|' + song.title;
    if (old && old.dataset.cfy === key) return;
    if (old) old.remove();
    const host = document.querySelector('ytd-watch-metadata #top-level-buttons-computed, ytd-watch-metadata #actions-inner, #above-the-fold #title');
    if (!host) return;
    const b = makeButton('play', song.artist, song.title, true);
    b.classList.add('cfy-yt');
    b.dataset.cfy = key;
    b.querySelector('span').textContent = 'open in chanceify';
    host.appendChild(b);
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
