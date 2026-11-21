#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! RunnerDeck: tray icon + dashboard + settings for the self-hosted Windows CI runners.
//!
//! Data comes from the local dashboard service (Dashboard-Server.ps1, http://127.0.0.1:8765/api/status),
//! which runs as SYSTEM and can see every runner account's processes. Settings are written straight
//! to the runner tooling's config\runners.json, which the supervisor rereads every cycle.

mod icon;

use icon::{tray_icon, Health};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    thread,
    time::Duration,
};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, State, WindowEvent, Wry,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

/// Where the runner tooling lives (it holds config/runners.json and state/). No default: set it in Settings.
const DEFAULT_TOOLS_ROOT: &str = "";
const DEFAULT_DASHBOARD: &str = "http://127.0.0.1:8765";
const TRAY_ID: &str = "main";
const MINIMIZED_ARG: &str = "--minimized";

// ------------------------------------------------------------------ app settings (per user)

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct AppSettings {
    tools_root: String,
    dashboard_url: String,
    poll_seconds: u64,
    /// show the small always-visible desktop widget
    widget: bool,
    /// keep the widget above other windows
    widget_on_top: bool,
    /// where the widget goes when the app starts: topLeft, topRight, bottomLeft, bottomRight,
    /// or remember (wherever it was last dragged)
    widget_corner: String,
    /// the widget has been placed once (used by "remember": the first start still needs a spot)
    widget_placed: bool,
}

const WIDGET_CORNERS: [&str; 5] = ["topLeft", "topRight", "bottomLeft", "bottomRight", "remember"];

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            tools_root: DEFAULT_TOOLS_ROOT.into(),
            dashboard_url: DEFAULT_DASHBOARD.into(),
            poll_seconds: 2,
            widget: false,
            widget_on_top: false,
            widget_corner: "bottomRight".into(),
            widget_placed: false,
        }
    }
}

struct AppState {
    settings: Mutex<AppSettings>,
    last: Mutex<Option<Value>>,
    last_error: Mutex<Option<String>>,
    pause_item: Mutex<Option<CheckMenuItem<Wry>>>,
    widget_item: Mutex<Option<CheckMenuItem<Wry>>>,
    /// what the tray icon shows now: overall health and which runner layers are lit
    tray_look: Mutex<Option<(Health, [bool; 3])>>,
}

fn settings_file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("settings.json"))
}

fn load_settings(app: &AppHandle) -> AppSettings {
    settings_file(app)
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn persist_settings(app: &AppHandle, s: &AppSettings) -> Result<(), String> {
    let file = settings_file(app).ok_or("no app config folder")?;
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(&file, serde_json::to_string_pretty(s).unwrap()).map_err(|e| e.to_string())
}

// ------------------------------------------------------------------ desktop widget

/// Move the widget into a corner of the primary screen's work area (inside the taskbar/menu bar).
fn place_widget(w: &tauri::WebviewWindow, corner: &str) {
    let monitor = w.primary_monitor().ok().flatten().or_else(|| w.current_monitor().ok().flatten());
    if let (Some(m), Ok(size)) = (monitor, w.outer_size()) {
        let area = m.work_area();
        let margin = (24.0 * m.scale_factor()) as i32;
        let left = area.position.x + margin;
        let right = area.position.x + area.size.width as i32 - size.width as i32 - margin;
        let top = area.position.y + margin;
        let bottom = area.position.y + area.size.height as i32 - size.height as i32 - margin;
        let (x, y) = match corner {
            "topLeft" => (left, top),
            "topRight" => (right, top),
            "bottomLeft" => (left, bottom),
            _ => (right, bottom),
        };
        let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
    }
}

/// Show or hide the widget per the settings. `reposition`: snap it to the chosen corner (done at
/// start-up and when the corner setting changes; "remember" keeps where it was last dragged).
fn apply_widget(app: &AppHandle, reposition: bool) {
    let Some(w) = app.get_webview_window("widget") else { return };
    let state = app.state::<AppState>();
    let mut s = state.settings.lock().unwrap();
    if !s.widget {
        let _ = w.hide();
    } else {
        if s.widget_corner == "remember" {
            if !s.widget_placed {
                place_widget(&w, "bottomRight");
            }
        } else if reposition || !s.widget_placed {
            place_widget(&w, &s.widget_corner);
        }
        if !s.widget_placed {
            s.widget_placed = true;
            let _ = persist_settings(app, &s);
        }
        let _ = w.set_always_on_top(s.widget_on_top);
        let _ = w.show();
    }
    if let Some(item) = state.widget_item.lock().unwrap().as_ref() {
        let _ = item.set_checked(s.widget);
    };
}

// ------------------------------------------------------------------ runner tooling paths

// The contract with the runner tooling (see README "Runner tooling contract"):
//   config/runners.json          settings the supervisor rereads every cycle
//   state/PAUSE                  pause all GitHub slots
//   state/STOP-slot-<n>          stop one GitHub slot
//   state/STOP-teamcity          stop the TeamCity agent
//   state/prune-request.json     ask the supervisor to prune; it answers in state/prune-result.json
fn config_path(root: &str) -> PathBuf {
    Path::new(root).join("config").join("runners.json")
}

fn state_file(root: &str, name: &str) -> PathBuf {
    Path::new(root).join("state").join(name)
}

fn pause_path(root: &str) -> PathBuf {
    state_file(root, "PAUSE")
}

fn tools_root(state: &State<AppState>) -> Result<String, String> {
    let root = state.settings.lock().unwrap().tools_root.clone();
    if root.trim().is_empty() {
        return Err("Set the runner tooling folder in Settings first.".into());
    }
    Ok(root)
}

/// Create (on = true) or remove a request flag file. Returns whether it exists afterwards.
fn set_flag(path: &Path, on: bool, note: &str) -> Result<bool, String> {
    if on {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        fs::write(path, format!("{note}\n")).map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    } else if path.exists() {
        fs::remove_file(path).map_err(|e| format!("cannot remove {}: {e}", path.display()))?;
    }
    Ok(path.exists())
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("{} is not valid JSON: {e}", path.display()))
}

/// Write via a temp file + rename, so the supervisor never reads a half-written config.
fn write_json_atomic(path: &Path, v: &Value) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(v).map_err(|e| e.to_string())?;
    fs::write(&tmp, text).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("cannot replace {}: {e}", path.display()))
}

// ------------------------------------------------------------------ status polling + tray

fn fetch_status(base: &str) -> Result<Value, String> {
    let url = format!("{}/api/status", base.trim_end_matches('/'));
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(3)).build();
    let body = agent
        .get(&url)
        .call()
        .map_err(|e| format!("dashboard service not reachable ({e})"))?
        .into_string()
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&body).map_err(|e| format!("bad status JSON: {e}"))
}

fn short_name(id: &str) -> String {
    match id {
        "tc" => "TC".into(),
        other => other.to_uppercase(),
    }
}

/// Which tray layer a runner lights: GitHub 1, GitHub 2, TeamCity (top to bottom).
fn layer_of(id: &str, index: usize) -> Option<usize> {
    match id {
        "gh1" => Some(0),
        "gh2" => Some(1),
        "tc" => Some(2),
        _ if index < 3 => Some(index),
        _ => None,
    }
}

/// Tooltip text (Windows caps tray tooltips at 127 characters), the overall health, and which tray
/// layers are lit.
fn summarize(v: &Value) -> (String, Health, [bool; 3]) {
    let cpu = v["machine"]["cpu"].as_f64().unwrap_or(0.0);
    let used = v["cap"]["usedGB"].as_f64().unwrap_or(0.0);
    let cap = v["cap"]["capGB"].as_f64().unwrap_or(10.0);
    let soft = v["cap"]["softGB"].as_f64().unwrap_or(cap);
    let paused = v["paused"].as_bool().unwrap_or(false);

    let mut warn = paused || used > soft;
    let mut present = [false; 3];
    let mut running = [false; 3];
    let mut parts = Vec::new();
    for (i, r) in v["runners"].as_array().into_iter().flatten().enumerate() {
        let id = r["id"].as_str().unwrap_or("?");
        let state = r["state"].as_str().unwrap_or("").to_lowercase();
        let is_running = state.starts_with("running");
        let word = if is_running {
            "Busy"
        } else if state.starts_with("idle") {
            "Idle"
        } else {
            warn = true;
            "Off"
        };
        if let Some(l) = layer_of(id, i) {
            present[l] = true;
            running[l] = is_running;
        }
        parts.push(format!("{} {} {:.0}%", short_name(id), word, r["cpu"].as_f64().unwrap_or(0.0)));
    }
    let busy = running.iter().any(|b| *b);
    let health = if used > cap {
        Health::Down
    } else if busy {
        Health::Busy
    } else if warn {
        Health::Warn
    } else {
        Health::Idle
    };
    // busy: only the running runners stay lit, so you see which one is working
    let lit = if busy { running } else { present };
    let mut tip = format!(
        "RunnerDeck{} · CPU {:.0}% · C: {:.1}/{:.0} GB\n{}",
        if paused { " (Paused)" } else { "" },
        cpu,
        used,
        cap,
        parts.join(" · ")
    );
    if tip.chars().count() > 127 {
        tip = tip.chars().take(126).collect::<String>() + "…";
    }
    (tip, health, lit)
}

fn set_tray_icon(app: &AppHandle, health: Health, lit: [bool; 3], pulse: bool) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_icon(Some(tray_icon(health, lit, pulse)));
        // macOS menu bar: a short word next to the icon (Windows has no tray title)
        #[cfg(target_os = "macos")]
        let _ = tray.set_title(Some(match health {
            Health::Idle => "Idle",
            Health::Busy => "Busy",
            Health::Warn => "Check",
            Health::Down => "Down",
        }));
    }
}

/// Update the tooltip every poll; redraw the icon only when the look changes (the pulse thread
/// animates it while something runs).
fn update_tray(app: &AppHandle, tip: &str, health: Health, lit: [bool; 3]) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(tip));
    }
    let state = app.state::<AppState>();
    let mut look = state.tray_look.lock().unwrap();
    if *look != Some((health, lit)) {
        *look = Some((health, lit));
        set_tray_icon(app, health, lit, false);
    }
}

/// While any runner is busy, alternate the badge between its two frames.
fn start_pulse(app: AppHandle) {
    thread::spawn(move || {
        let mut frame = false;
        loop {
            thread::sleep(Duration::from_millis(700));
            let look = *app.state::<AppState>().tray_look.lock().unwrap();
            if let Some((Health::Busy, lit)) = look {
                frame = !frame;
                set_tray_icon(&app, Health::Busy, lit, frame);
            } else {
                frame = false;
            }
        }
    });
}

fn start_polling(app: AppHandle) {
    thread::spawn(move || loop {
        let state = app.state::<AppState>();
        let s = state.settings.lock().unwrap().clone();
        match fetch_status(&s.dashboard_url) {
            Ok(v) => {
                let (tip, health, lit) = summarize(&v);
                update_tray(&app, &tip, health, lit);
                *state.last.lock().unwrap() = Some(v);
                *state.last_error.lock().unwrap() = None;
            }
            Err(e) => {
                update_tray(&app, "RunnerDeck · Dashboard Service Not Reachable", Health::Down, [false; 3]);
                *state.last_error.lock().unwrap() = Some(e);
            }
        }
        // keep the tray's pause tick in step with the real PAUSE file
        if let Some(item) = state.pause_item.lock().unwrap().as_ref() {
            let on = !s.tools_root.trim().is_empty() && pause_path(&s.tools_root).exists();
            let _ = item.set_checked(on);
        };
        thread::sleep(Duration::from_secs(s.poll_seconds.clamp(1, 60)));
    });
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn open_in_browser(url: &str) -> Result<(), String> {
    // only ever the local dashboard
    if !(url.starts_with("http://127.0.0.1:") || url.starts_with("http://localhost:")) {
        return Err("only the local dashboard can be opened".into());
    }
    #[cfg(windows)]
    let mut cmd = std::process::Command::new("explorer");
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = std::process::Command::new("xdg-open");
    cmd.arg(url).spawn().map(|_| ()).map_err(|e| e.to_string())
}

fn write_pause(root: &str, paused: bool) -> Result<bool, String> {
    set_flag(&pause_path(root), paused, "paused from RunnerDeck")
}

// ------------------------------------------------------------------ commands

#[tauri::command]
fn get_status(state: State<AppState>) -> Result<Value, String> {
    if let Some(e) = state.last_error.lock().unwrap().clone() {
        return Err(e);
    }
    state.last.lock().unwrap().clone().ok_or_else(|| "waiting for the first sample…".into())
}

#[tauri::command]
fn get_app_settings(state: State<AppState>) -> AppSettings {
    state.settings.lock().unwrap().clone()
}

/// The fields the Settings page edits (the widget has its own command).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppSettingsPatch {
    tools_root: String,
    dashboard_url: String,
    poll_seconds: u64,
}

#[tauri::command]
fn save_app_settings(app: AppHandle, state: State<AppState>, settings: AppSettingsPatch) -> Result<(), String> {
    let url = settings.dashboard_url.trim().trim_end_matches('/').to_string();
    if !(url.starts_with("http://127.0.0.1:") || url.starts_with("http://localhost:")) {
        return Err("The dashboard service address must be http://127.0.0.1:<port> or http://localhost:<port>.".into());
    }
    let root = settings.tools_root.trim().to_string();
    if !root.is_empty() && !config_path(&root).exists() {
        return Err(format!("No runner config at {}.", config_path(&root).display()));
    }
    if !(1..=60).contains(&settings.poll_seconds) {
        return Err("Refresh interval must be 1 to 60 seconds.".into());
    }
    let mut s = state.settings.lock().unwrap();
    s.tools_root = root;
    s.dashboard_url = url;
    s.poll_seconds = settings.poll_seconds;
    persist_settings(&app, &s)
}

#[tauri::command]
fn set_widget(app: AppHandle, state: State<AppState>, visible: bool, on_top: bool, corner: String) -> Result<(), String> {
    if !WIDGET_CORNERS.contains(&corner.as_str()) {
        return Err(format!("Unknown widget position '{corner}'."));
    }
    let moved = {
        let mut s = state.settings.lock().unwrap();
        let moved = s.widget_corner != corner || (visible && !s.widget);
        s.widget = visible;
        s.widget_on_top = on_top;
        s.widget_corner = corner;
        persist_settings(&app, &s)?;
        moved
    };
    // snap to the corner right away when the position is changed or the widget is switched on
    apply_widget(&app, moved);
    Ok(())
}

#[tauri::command]
fn show_main_window(app: AppHandle) {
    show_main(&app);
}

#[tauri::command]
fn get_runner_config(state: State<AppState>) -> Result<Value, String> {
    let root = tools_root(&state)?;
    let c = read_json(&config_path(&root))?;
    Ok(json!({
        "org": c["org"],
        "runnerGroup": c["runnerGroup"],
        "labels": c["labels"],
        "slots": c["slots"],
        "runnersAreAdmins": c["runnersAreAdmins"],
        "runnerVersion": c["runner"]["version"],
        "autoUpdatePackage": c["runner"]["autoUpdatePackage"],
        "budget": c["budget"],
        "paused": pause_path(&root).exists(),
        "configPath": config_path(&root).display().to_string(),
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunnerPatch {
    hard_cap_gb: f64,
    soft_gb: f64,
    min_free_to_start_gb: f64,
    log_retention_days: u32,
    watchdog_seconds: u32,
    auto_update_package: bool,
}

/// Whole numbers stay integers in the JSON, so the file keeps the shape PowerShell wrote.
fn num(f: f64) -> Value {
    if f.fract() == 0.0 { json!(f as i64) } else { json!((f * 100.0).round() / 100.0) }
}

#[tauri::command]
fn save_runner_config(state: State<AppState>, patch: RunnerPatch) -> Result<(), String> {
    let p = &patch;
    if !(2.0..=500.0).contains(&p.hard_cap_gb) {
        return Err("Hard cap must be between 2 and 500 GB.".into());
    }
    if !(p.soft_gb >= 1.0 && p.soft_gb < p.hard_cap_gb) {
        return Err("Prune target must be at least 1 GB and below the hard cap.".into());
    }
    if !(p.min_free_to_start_gb >= 0.5 && p.min_free_to_start_gb < p.hard_cap_gb) {
        return Err("Room needed per job must be at least 0.5 GB and below the hard cap.".into());
    }
    if !(1..=90).contains(&p.log_retention_days) {
        return Err("Log retention must be 1 to 90 days.".into());
    }
    if !(10..=300).contains(&p.watchdog_seconds) {
        return Err("Check interval must be 10 to 300 seconds.".into());
    }
    let root = tools_root(&state)?;
    let path = config_path(&root);
    let mut c = read_json(&path)?;
    let b = c.get_mut("budget").and_then(Value::as_object_mut).ok_or("config has no budget section")?;
    b.insert("hardCapGB".into(), num(p.hard_cap_gb));
    b.insert("softGB".into(), num(p.soft_gb));
    b.insert("minFreeToStartGB".into(), num(p.min_free_to_start_gb));
    b.insert("logRetentionDays".into(), json!(p.log_retention_days));
    b.insert("watchdogSeconds".into(), json!(p.watchdog_seconds));
    let r = c.get_mut("runner").and_then(Value::as_object_mut).ok_or("config has no runner section")?;
    r.insert("autoUpdatePackage".into(), json!(p.auto_update_package));
    write_json_atomic(&path, &c)
}

#[tauri::command]
fn set_paused(state: State<AppState>, paused: bool) -> Result<bool, String> {
    let root = tools_root(&state)?;
    let now = write_pause(&root, paused)?;
    if let Some(item) = state.pause_item.lock().unwrap().as_ref() {
        let _ = item.set_checked(now);
    }
    Ok(now)
}

#[tauri::command]
fn get_controls(state: State<AppState>) -> Result<Value, String> {
    let root = tools_root(&state)?;
    let c = read_json(&config_path(&root))?;
    let slots: Vec<Value> = c["slots"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_i64)
        .map(|s| json!({ "slot": s, "stopped": state_file(&root, &format!("STOP-slot-{s}")).exists() }))
        .collect();
    let result = read_json(&state_file(&root, "prune-result.json")).ok();
    Ok(json!({
        "paused": pause_path(&root).exists(),
        "slots": slots,
        "teamcityStopped": state_file(&root, "STOP-teamcity").exists(),
        "prunePending": state_file(&root, "prune-request.json").exists(),
        "pruneResult": result,
    }))
}

#[tauri::command]
fn set_slot_running(state: State<AppState>, slot: u32, running: bool) -> Result<bool, String> {
    let root = tools_root(&state)?;
    let c = read_json(&config_path(&root))?;
    if !c["slots"].as_array().into_iter().flatten().any(|s| s.as_u64() == Some(slot as u64)) {
        return Err(format!("There is no GitHub slot {slot}."));
    }
    let stopped = set_flag(&state_file(&root, &format!("STOP-slot-{slot}")), !running, "stopped from RunnerDeck")?;
    Ok(!stopped)
}

#[tauri::command]
fn set_teamcity_running(state: State<AppState>, running: bool) -> Result<bool, String> {
    let root = tools_root(&state)?;
    let stopped = set_flag(&state_file(&root, "STOP-teamcity"), !running, "stopped from RunnerDeck")?;
    Ok(!stopped)
}

#[tauri::command]
fn request_prune(state: State<AppState>, kind: String) -> Result<(), String> {
    let root = tools_root(&state)?;
    let (level, all) = match kind.as_str() {
        "quick" => (1, false),
        "deep" => (2, false),
        "deepAll" => (2, true),
        _ => return Err("unknown prune kind".into()),
    };
    let file = state_file(&root, "prune-request.json");
    if file.exists() {
        return Err("A prune is already queued; the supervisor picks it up within 30 seconds.".into());
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let req = json!({ "level": level, "targetGB": 0, "allDrives": all, "requestedAt": now, "from": "RunnerDeck" });
    write_json_atomic(&file, &req)
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let l = app.autolaunch();
    if enabled { l.enable() } else { l.disable() }.map_err(|e| e.to_string())?;
    l.is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
fn open_dashboard_in_browser(state: State<AppState>) -> Result<(), String> {
    let url = state.settings.lock().unwrap().dashboard_url.clone();
    open_in_browser(&format!("{}/", url.trim_end_matches('/')))
}

// ------------------------------------------------------------------ app

fn main() {
    tauri::Builder::default()
        // a second launch just brings the existing window forward (one tray icon, ever)
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec![MINIMIZED_ARG])))
        // remembers where the windows were (position only; visibility is ours to decide)
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(tauri_plugin_window_state::StateFlags::POSITION)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_app_settings,
            save_app_settings,
            get_runner_config,
            save_runner_config,
            set_paused,
            get_controls,
            set_slot_running,
            set_teamcity_running,
            request_prune,
            set_widget,
            show_main_window,
            get_autostart,
            set_autostart,
            open_dashboard_in_browser
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let settings = load_settings(&handle);
            let paused = !settings.tools_root.trim().is_empty() && pause_path(&settings.tools_root).exists();
            app.manage(AppState {
                settings: Mutex::new(settings),
                last: Mutex::new(None),
                last_error: Mutex::new(None),
                pause_item: Mutex::new(None),
                widget_item: Mutex::new(None),
                tray_look: Mutex::new(None),
            });
            let widget_on = app.state::<AppState>().settings.lock().unwrap().widget;

            let open_i = MenuItem::with_id(app, "open", "Open RunnerDeck", true, None::<&str>)?;
            let widget_i = CheckMenuItem::with_id(app, "widget", "Desktop Widget", true, widget_on, None::<&str>)?;
            let browser_i = MenuItem::with_id(app, "browser", "Open Dashboard In Browser", true, None::<&str>)?;
            let pause_i = CheckMenuItem::with_id(app, "pause", "Pause All GitHub Runners", true, paused, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit RunnerDeck", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[&open_i, &widget_i, &browser_i, &PredefinedMenuItem::separator(app)?, &pause_i, &PredefinedMenuItem::separator(app)?, &quit_i],
            )?;
            *app.state::<AppState>().pause_item.lock().unwrap() = Some(pause_i.clone());
            *app.state::<AppState>().widget_item.lock().unwrap() = Some(widget_i.clone());

            TrayIconBuilder::with_id(TRAY_ID)
                .icon(tray_icon(Health::Warn, [true; 3], false))
                .tooltip("RunnerDeck · Starting…")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "open" => show_main(app),
                    "widget" => {
                        let state = app.state::<AppState>();
                        {
                            let mut s = state.settings.lock().unwrap();
                            s.widget = !s.widget;
                            let _ = persist_settings(app, &s);
                        }
                        apply_widget(app, true);
                    }
                    "browser" => {
                        let url = app.state::<AppState>().settings.lock().unwrap().dashboard_url.clone();
                        let _ = open_in_browser(&format!("{}/", url.trim_end_matches('/')));
                    }
                    "pause" => {
                        let state = app.state::<AppState>();
                        let root = state.settings.lock().unwrap().tools_root.clone();
                        if root.trim().is_empty() {
                            show_main(app);
                            return;
                        }
                        let want = !pause_path(&root).exists();
                        let now = write_pause(&root, want).unwrap_or(!want);
                        if let Some(item) = state.pause_item.lock().unwrap().as_ref() {
                            let _ = item.set_checked(now);
                        };
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                        show_main(tray.app_handle());
                    }
                })
                .build(app)?;

            // closing the window keeps the app in the tray
            if let Some(w) = app.get_webview_window("main") {
                let w2 = w.clone();
                w.on_window_event(move |e| {
                    if let WindowEvent::CloseRequested { api, .. } = e {
                        api.prevent_close();
                        let _ = w2.hide();
                    }
                });
            }
            // closing the widget (Alt+F4) just turns it off
            if let Some(w) = app.get_webview_window("widget") {
                let h2 = handle.clone();
                let w2 = w.clone();
                w.on_window_event(move |e| {
                    if let WindowEvent::CloseRequested { api, .. } = e {
                        api.prevent_close();
                        let _ = w2.hide();
                        let state = h2.state::<AppState>();
                        let mut s = state.settings.lock().unwrap();
                        s.widget = false;
                        let _ = persist_settings(&h2, &s);
                        if let Some(item) = state.widget_item.lock().unwrap().as_ref() {
                            let _ = item.set_checked(false);
                        };
                    }
                });
            }
            apply_widget(&handle, true); // start-up: snap to the chosen corner
            // started by Windows at sign-in: stay in the tray; started by hand: show the window
            if !std::env::args().any(|a| a == MINIMIZED_ARG) {
                show_main(&handle);
            }
            start_pulse(handle.clone());
            start_polling(handle);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running RunnerDeck");
}
