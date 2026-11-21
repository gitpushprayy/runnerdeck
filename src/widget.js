// Desktop widget: a compact live view. Drag anywhere on the card; "Open" brings up the main window.
const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args);
const $ = id => document.getElementById(id);
const COLORS = { gh1: 'var(--s1)', gh2: 'var(--s2)', tc: 'var(--tc)' };
const SHORT = { gh1: 'GitHub 1', gh2: 'GitHub 2', gh3: 'GitHub 3', tc: 'TeamCity' };
let lastOk = 0;

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text != null) e.textContent = text;
  e.setAttribute('data-tauri-drag-region', '');   // every part of the card drags the window
  return e;
}
const shortState = s => {
  const l = (s || '').toLowerCase();
  if (l.startsWith('running build')) return ['Building', 'run'];
  if (l.startsWith('running')) return ['Running job', 'run'];
  if (l.startsWith('idle')) return ['Idle', ''];
  if (l.startsWith('restarting')) return ['Restarting', ''];
  return [s ? s[0].toUpperCase() + s.slice(1).split(' · ')[0] : '–', 'off'];
};
const barColor = p => p >= 60 ? 'var(--warn)' : 'var(--ok)';

function render(d) {
  const rows = $('rows');
  rows.replaceChildren();
  for (const r of d.runners) {
    const row = el('div', 'row'), sw = el('span', 'sw');
    sw.style.background = COLORS[r.id] || 'var(--s1)';
    const [st, cls] = shortState(r.state);
    const cpu = el('span', 'cpu'), bar = el('span', 'bar'), fill = el('i');
    // scale: one runner flat out on every core = 100%
    fill.style.width = Math.min(100, r.cpu || 0) + '%';
    fill.style.background = barColor(r.cpu || 0);
    bar.append(fill);
    cpu.append(bar, el('span', null, (r.cpu ?? 0).toFixed(0) + '%'));
    row.append(sw, el('span', 'name', SHORT[r.id] || r.label), el('span', 'state ' + cls, st), cpu);
    rows.append(row);
  }
  $('cpu').textContent = `CPU ${d.machine.cpu}% · RAM ${d.machine.ramUsedGB.toFixed(1)} GB`;
  $('disk').textContent = `Disk ${d.cap.usedGB.toFixed(1)} / ${d.cap.capGB} GB`;
}

async function tick() {
  try { render(await invoke('get_status')); lastOk = Date.now(); }
  catch (e) {
    if (!lastOk) { $('rows').replaceChildren(el('div', 'msg', 'Dashboard service not reachable')); }
  }
  $('dot').classList.toggle('stale', !lastOk || Date.now() - lastOk > 10000);
}
$('open').addEventListener('click', () => invoke('show_main_window'));
tick(); setInterval(tick, 2000);
