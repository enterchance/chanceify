// chanceify for Last.fm: adds a one-click play button to songs on Last.fm.
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
    `chanceify://${mode}?artist=${encodeURIComponent(artist)}&track=${encodeURIComponent(track)}`;

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
    toast(mode === 'queue' ? `added to queue: ${track}` : `playing in chanceify: ${track}`);
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

  function apply() {
    document.documentElement.classList.toggle('cfy-hide-play', !!opts.replace);
    try { decorateRows(); decorateTrackPage(); } catch (e) { /* never break the page */ }
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
