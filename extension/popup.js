const api = typeof browser !== 'undefined' ? browser : chrome;
const defaults = { replace: true, toast: true };
for (const id of Object.keys(defaults)) {
  const el = document.getElementById(id);
  api.storage.sync.get(defaults, (v) => { el.checked = !!v[id]; });
  el.addEventListener('change', () => api.storage.sync.set({ [id]: el.checked }));
}
