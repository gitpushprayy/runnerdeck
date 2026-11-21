// RunnerDeck frontend. All data and actions go through Tauri commands (src-tauri/src/main.rs).
const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args);
const $ = id => document.getElementById(id);
const fmt = (n, d = 1) => (n == null || isNaN(n)) ? '–' : Number(n).toFixed(d);
const COLORS = { gh1: 'var(--s1)', gh2: 'var(--s2)', gh3: 'var(--warn)', tc: 'var(--tc)', machine: 'var(--m)' };
const isMac = /Mac/i.test(navigator.platform || navigator.userAgent);
let lastStatus = null, lastOk = 0, lastErr = '';

const cap1 = s => s ? s[0].toUpperCase() + s.slice(1) : s;
function el(tag, cls, text) { const e = document.createElement(tag); if (cls) e.className = cls; if (text != null) e.textContent = text; return e; }
function setMsg(id, text, ok) { const m = $(id); m.textContent = text || ''; m.className = 'msg ' + (text ? (ok ? 'ok' : 'err') : ''); }
const pctColor = p => p >= 90 ? 'var(--bad)' : p >= 70 ? 'var(--warn)' : 'var(--accent)';
function setBar(e, pct, color) { e.style.width = Math.max(0, Math.min(100, pct || 0)) + '%'; e.style.background = color || pctColor(pct); }

// ------------------------------------------------------------------ tabs
document.querySelectorAll('nav.tabs button').forEach(b => b.addEventListener('click', () => showTab(b.dataset.tab)));
function showTab(name) {
  document.querySelectorAll('nav.tabs button').forEach(b => b.setAttribute('aria-selected', String(b.dataset.tab === name)));
  for (const t of ['dashboard', 'controls', 'settings']) $('tab-' + t).hidden = t !== name;
  if (name === 'controls') refreshControls();
  if (name === 'settings') loadSettings();
  try { localStorage.setItem('tab', name); } catch {}
}

// ------------------------------------------------------------------ dashboard
function path(values, w, h, max) {
  if (!values.length) return '';
  const step = values.length > 1 ? w / (values.length - 1) : w;
  return values.map((v, i) => `${(i * step).toFixed(1)},${(h - (Math.min(v ?? 0, max) / max) * h).toFixed(1)}`).join(' ');
}
function spark(values, color) {
  const w = 300, h = 44, pts = path(values, w, h - 2, 100);
  return `<svg class="spark" viewBox="0 0 ${w} ${h}" preserveAspectRatio="none">
    <polygon points="${pts ? `0,${h} ${pts} ${w},${h}` : ''}" fill="${color}" opacity=".12"></polygon>
    <polyline points="${pts}" fill="none" stroke="${color}" stroke-width="1.8" vector-effect="non-scaling-stroke" stroke-linejoin="round"></polyline></svg>`;
}
const stateClass = s => /^running/i.test(s) ? 'run' : /^idle/i.test(s) ? '' : 'off';

function renderDashboard(d) {
  $('host').replaceChildren(el('span', 'nt', d.host), document.createTextNode(` · ${d.machine.cores} cores`));
  $('cpu').textContent = fmt(d.machine.cpu, 0); setBar($('cpuBar'), d.machine.cpu);
  $('cores').textContent = `Runners use ${fmt(d.runners.reduce((a, r) => a + (r.cpu || 0), 0))}% of it`;
  $('ram').textContent = fmt(d.machine.ramUsedGB); setBar($('ramBar'), d.machine.ramUsedGB / d.machine.ramTotalGB * 100);
  $('ramMeta').textContent = `Of ${fmt(d.machine.ramTotalGB)} GB · Runners ${fmt(d.runners.reduce((a, r) => a + (r.ramMB || 0), 0) / 1024)} GB`;
  $('cap').textContent = fmt(d.cap.usedGB, 2);
  setBar($('capBar'), d.cap.usedGB / d.cap.capGB * 100, d.cap.usedGB > d.cap.capGB ? 'var(--bad)' : d.cap.usedGB > d.cap.softGB ? 'var(--warn)' : 'var(--ok)');
  $('capMeta').textContent = `Of ${d.cap.capGB} GB cap (prunes above ${d.cap.softGB} GB) · Drive has ${fmt(d.cap.driveFreeGB, 0)} GB free`;

  const msgs = [];
  if (d.paused) msgs.push('All GitHub runners are paused.');
  if (d.controls && d.controls.stoppedSlots && d.controls.stoppedSlots.length) msgs.push(`GitHub slot ${d.controls.stoppedSlots.join(', ')} stopped.`);
  if (d.controls && d.controls.teamcityStopped) msgs.push('TeamCity agent stopped.');
  if (d.cap.usedGB > d.cap.capGB) msgs.push('Over the hard cap: pruning, and the largest GitHub job may be stopped.');
  $('banner').hidden = !msgs.length; $('banner').textContent = msgs.join('  ');

  const host = $('runners'); host.replaceChildren();
  for (const r of d.runners) {
    const c = el('div', 'card'), head = el('div', 'runner-head'), sw = el('span', 'swatch');
    sw.style.background = COLORS[r.id] || 'var(--accent)';
    head.append(sw, el('span', 'rname', r.label), el('span', 'pill ' + stateClass(r.state), r.state));
    c.append(head, el('div', 'regname', r.name || '—'));
    const kv = el('div', 'kv'), a = el('div'), b = el('div');
    const coresUsed = (r.cpu || 0) / 100 * d.machine.cores;
    a.append(el('div', 'v', fmt(r.cpu) + '%'), el('div', 'k', 'CPU'));
    b.append(el('div', 'v', r.ramMB == null ? '–' : (r.ramMB >= 1024 ? fmt(r.ramMB / 1024) + ' GB' : Math.round(r.ramMB) + ' MB')), el('div', 'k', 'RAM'));
    kv.append(a, b); c.append(kv);
    // the mini graph scales to this runner's own peak, so one busy core is still visible on a 24-core machine
    const hist = (d.history[r.id] || []).slice(-90), peak = Math.max(10, Math.ceil(Math.max(0, ...hist) * 1.2 / 5) * 5);
    const sp = document.createElement('div'); sp.innerHTML = spark(hist.map(v => v * 100 / peak), COLORS[r.id] || 'var(--accent)'); c.append(sp);
    c.append(el('div', 'meta', `${fmt(coresUsed)} of ${d.machine.cores} cores · Graph 0–${peak}%, last 3 min`));
    c.append(el('div', 'meta', r.busiest && r.busiest !== '-' ? `Busiest: ${r.busiest} · ${r.processes} processes` : `${r.processes ?? 0} processes`));
    host.append(c);
  }

  const W = 600, H = 220, pad = 20, series = [['machine', 'Machine total'], ...d.runners.map(r => [r.id, r.label])];
  let s = '';
  for (const g of [0, 25, 50, 75, 100]) { const y = (H - pad) - g / 100 * (H - pad - 6); s += `<line class="gridline" x1="28" x2="${W}" y1="${y}" y2="${y}"></line><text class="axis" x="0" y="${y + 4}">${g}%</text>`; }
  for (const [id] of series) {
    const pts = path(d.history[id] || [], W - 30, H - pad - 6, 100).split(' ').filter(Boolean)
      .map(p => { const [x, y] = p.split(',').map(Number); return `${(x + 30).toFixed(1)},${(y + 6).toFixed(1)}`; }).join(' ');
    s += `<polyline points="${pts}" fill="none" stroke="${COLORS[id] || 'var(--accent)'}" stroke-width="${id === 'machine' ? 1.4 : 2}" ${id === 'machine' ? 'stroke-dasharray="4 3"' : ''} vector-effect="non-scaling-stroke" stroke-linejoin="round"></polyline>`;
  }
  $('chart').innerHTML = s; $('histMin').textContent = d.historyMinutes;
  const lg = $('legend'); lg.replaceChildren();
  for (const [id, name] of series) { const sp = el('span'), w = el('span', 'swatch'); w.style.background = COLORS[id] || 'var(--accent)'; sp.append(w, document.createTextNode(name)); lg.append(sp); }

  const it = $('items'); it.replaceChildren();
  for (const i of d.cap.items) { const tr = el('tr'); tr.append(el('td', null, i.name), el('td', null, i.gb >= 1 ? fmt(i.gb, 2) + ' GB' : Math.round(i.gb * 1024) + ' MB')); it.append(tr); }
  const gh = $('gh'); gh.replaceChildren();
  if (d.github.error) { const tr = el('tr'); tr.append(el('td', null, d.github.error), el('td')); gh.append(tr); }
  for (const g of d.github.runners) { const tr = el('tr'); tr.append(el('td', null, g.name), el('td', null, cap1(g.status) + (g.busy ? ' · busy' : ''))); gh.append(tr); }
  if (!d.github.error && !d.github.runners.length) { const tr = el('tr'); tr.append(el('td', null, 'None registered right now'), el('td')); gh.append(tr); }
  const log = $('log'); log.replaceChildren();
  for (const line of d.log) { const m = line.match(/\[(ERROR|WARN|INFO)\]/); log.append(el('div', m ? m[1] : '', line)); }
}

async function tick() {
  try {
    const d = await invoke('get_status');
    lastStatus = d; lastOk = Date.now(); lastErr = '';
    renderDashboard(d);
    if (!$('tab-controls').hidden) renderControlStates(d);
  } catch (e) { lastErr = String(e); }
  const age = (Date.now() - lastOk) / 1000, stale = !lastOk || age > 10;
  $('dot').classList.toggle('stale', stale);
  $('updated').textContent = !lastOk ? (lastErr ? 'No data' : 'Connecting…') : stale ? `No data for ${Math.round(age)}s` : 'Live · ' + new Date(lastOk).toLocaleTimeString();
  $('downBanner').hidden = !(stale && lastErr);
  $('downBanner').textContent = lastErr ? `Can't reach the dashboard service: ${lastErr}` : '';
}

// ------------------------------------------------------------------ controls
let controls = null;
async function refreshControls() {
  try { controls = await invoke('get_controls'); } catch (e) { setMsg('pruneMsg', String(e), false); return; }
  $('pauseAll').checked = controls.paused;
  const rows = $('slotRows'); rows.replaceChildren();
  for (const s of controls.slots) {
    const row = el('div', 'row'), what = el('div', 'what');
    what.append(el('b', null, `GitHub slot ${s.slot}`), el('span', null, '–'));
    what.lastChild.id = `slotState${s.slot}`;
    const btn = el('button', 'btn', s.stopped ? 'Start' : 'Stop');
    btn.addEventListener('click', async () => {
      btn.disabled = true;
      try { await invoke('set_slot_running', { slot: s.slot, running: s.stopped }); } catch (e) { alert(e); }
      await refreshControls(); btn.disabled = false;
    });
    row.append(what, btn); rows.append(row);
  }
  $('tcBtn').textContent = controls.teamcityStopped ? 'Start' : 'Stop';
  const r = controls.pruneResult;
  if (controls.prunePending) setMsg('pruneMsg', 'Clean-up queued; the supervisor runs it within about 30 seconds…', true);
  else if (r) setMsg('pruneMsg', `Last clean-up ${new Date(r.finishedAt).toLocaleString()}: runner disk ${fmt(r.capBeforeGB, 2)} → ${fmt(r.capAfterGB, 2)} GB` + (r.allDrives ? `, shared caches freed ${fmt(r.otherFreedGB, 2)} GB` : '') + (r.note ? ` (${r.note})` : '') + '.', true);
  if (lastStatus) renderControlStates(lastStatus);
}
function renderControlStates(d) {
  for (const r of d.runners) {
    if (r.id === 'tc') { $('tcState').textContent = r.state; if (r.name) $('tcName').textContent = r.name; }
    else {
      const e = $(`slotState${r.id.slice(2)}`);
      if (e) { e.replaceChildren(document.createTextNode(r.state + (r.name ? ' · ' : ''))); if (r.name) e.append(el('span', 'nt', r.name)); }
    }
  }
  $('tcCard').hidden = !d.runners.some(r => r.id === 'tc');
}
$('pauseAll').addEventListener('change', async e => {
  try { await invoke('set_paused', { paused: e.target.checked }); } catch (err) { alert(err); }
  refreshControls();
});
$('tcBtn').addEventListener('click', async () => {
  try { await invoke('set_teamcity_running', { running: controls.teamcityStopped }); } catch (e) { alert(e); }
  refreshControls();
});
document.querySelectorAll('[data-prune]').forEach(b => b.addEventListener('click', async () => {
  const kind = b.dataset.prune;
  if (kind === 'deepAll' && !confirm('Clear all shared caches? The next build of each project will download and compile dependencies again.')) return;
  try { await invoke('request_prune', { kind }); setMsg('pruneMsg', 'Clean-up queued; the supervisor runs it within about 30 seconds…', true); }
  catch (e) { setMsg('pruneMsg', String(e), false); }
  pollPrune();
}));
let prunePoll = null;
function pollPrune() {
  clearInterval(prunePoll);
  let n = 0;
  prunePoll = setInterval(async () => { await refreshControls(); if ((controls && !controls.prunePending) || ++n > 60) clearInterval(prunePoll); }, 3000);
}

// ------------------------------------------------------------------ settings
async function loadSettings() {
  setMsg('appMsg'); setMsg('runnerMsg');
  try {
    const a = await invoke('get_app_settings');
    $('toolsRoot').value = a.toolsRoot; $('dashUrl').value = a.dashboardUrl; $('poll').value = a.pollSeconds;
    $('widget').checked = !!a.widget; $('widgetTop').checked = !!a.widgetOnTop; $('widgetCorner').value = a.widgetCorner || 'bottomRight';
    $('autostart').checked = await invoke('get_autostart');
  } catch (e) { setMsg('appMsg', String(e), false); }
  try {
    const c = await invoke('get_runner_config');
    $('hardCap').value = c.budget.hardCapGB; $('soft').value = c.budget.softGB; $('minFree').value = c.budget.minFreeToStartGB;
    $('logDays').value = c.budget.logRetentionDays; $('watchdog').value = c.budget.watchdogSeconds; $('autoUpdate').checked = !!c.autoUpdatePackage;
    $('cfgPath').textContent = c.configPath;
    const facts = $('facts'); facts.replaceChildren();
    const add = (k, v) => { facts.append(el('dt', null, k), el('dd', null, v == null ? '–' : String(v))); };
    add('GitHub org', c.org); add('Runner group', c.runnerGroup); add('Labels', ['self-hosted', ...(c.labels || [])].join(', '));
    add('GitHub slots', (c.slots || []).join(', ')); add('Runner version', c.runnerVersion);
    add('Runner accounts are admins', c.runnersAreAdmins ? 'yes' : 'no');
  } catch (e) { setMsg('runnerMsg', String(e), false); }
}
$('autostart').addEventListener('change', async e => {
  try { e.target.checked = await invoke('set_autostart', { enabled: e.target.checked }); setMsg('appMsg', e.target.checked ? 'Starts at sign-in.' : 'Won’t start at sign-in.', true); }
  catch (err) { setMsg('appMsg', String(err), false); e.target.checked = !e.target.checked; }
});
async function saveWidget() {
  try {
    await invoke('set_widget', { visible: $('widget').checked, onTop: $('widgetTop').checked, corner: $('widgetCorner').value });
    setMsg('appMsg', 'Widget updated.', true);
  } catch (e) { setMsg('appMsg', String(e), false); }
}
$('widget').addEventListener('change', saveWidget);
$('widgetTop').addEventListener('change', saveWidget);
$('widgetCorner').addEventListener('change', saveWidget);
$('saveApp').addEventListener('click', async () => {
  try {
    await invoke('save_app_settings', { settings: { toolsRoot: $('toolsRoot').value, dashboardUrl: $('dashUrl').value, pollSeconds: Number($('poll').value) } });
    setMsg('appMsg', 'Saved.', true); loadSettings();
  } catch (e) { setMsg('appMsg', String(e), false); }
});
$('openBrowser').addEventListener('click', () => invoke('open_dashboard_in_browser').catch(e => setMsg('appMsg', String(e), false)));
$('saveRunner').addEventListener('click', async () => {
  const patch = {
    hardCapGb: Number($('hardCap').value), softGb: Number($('soft').value), minFreeToStartGb: Number($('minFree').value),
    logRetentionDays: Number($('logDays').value), watchdogSeconds: Number($('watchdog').value), autoUpdatePackage: $('autoUpdate').checked,
  };
  try { await invoke('save_runner_config', { patch }); setMsg('runnerMsg', 'Saved. The supervisor applies it within about 30 seconds.', true); }
  catch (e) { setMsg('runnerMsg', String(e), false); }
});

// ------------------------------------------------------------------ start
if (isMac) document.querySelector('#tab-settings .row .what b').textContent = 'Start at login';
let startTab = 'dashboard';
try { startTab = localStorage.getItem('tab') || 'dashboard'; } catch {}
showTab(startTab);
tick(); setInterval(tick, 2000);
