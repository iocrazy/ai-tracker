use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use tauri::{
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, RunEvent, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::CommandChild;
#[cfg(target_os = "macos")]
use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};

/// Global sidecar PID for signal handler cleanup.
/// When the app is killed via SIGTERM/SIGINT (e.g. pkill), the RunEvent::ExitRequested
/// handler doesn't fire. This atomic lets the signal handler kill the sidecar.
static SIDECAR_PID: AtomicU32 = AtomicU32::new(0);

extern "C" fn signal_cleanup(_sig: libc::c_int) {
    let pid = SIDECAR_PID.swap(0, Ordering::Relaxed);
    if pid != 0 {
        unsafe { libc::kill(pid as i32, libc::SIGKILL); }
    }
    unsafe { libc::_exit(1); }
}

fn install_signal_handlers() {
    unsafe {
        // Use sigaction for reliable signal handling that persists
        // across framework event loops
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = signal_cleanup as *const () as libc::sighandler_t;
        sa.sa_flags = 0;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(libc::SIGTERM, &sa, std::ptr::null_mut());
        libc::sigaction(libc::SIGINT, &sa, std::ptr::null_mut());
        libc::sigaction(libc::SIGHUP, &sa, std::ptr::null_mut());
    }
}

/// Holds the sidecar child process so we can kill it on exit.
struct SidecarState {
    child: Mutex<Option<CommandChild>>,
    /// "sidecar" if we spawned it, "external" if port was already occupied
    source: &'static str,
}

/// Legacy config directory (~/.config/agent-tracker).
/// Single source of truth for the Tauri crate; mirrors TrackerPaths::legacy_config_dir() in the server.
fn legacy_config_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    std::path::PathBuf::from(home).join(".config").join("agent-tracker")
}

/// Check if a TCP port is already in use.
fn is_port_in_use(port: u16) -> bool {
    std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
}

/// Start the tracker-server sidecar if port 3099 is not already occupied.
fn start_sidecar(app: &tauri::AppHandle) {
    const PORT: u16 = 3099;

    if is_port_in_use(PORT) {
        eprintln!("tracker-server already running on port {PORT}, reusing existing instance");
        app.manage(SidecarState {
            child: Mutex::new(None),
            source: "external",
        });
        return;
    }

    // Resolve Tauri standard directories for sidecar env vars
    let resources_dir = app.path().resource_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let data_dir = app.path().app_data_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    // Ensure data directory exists
    if !data_dir.is_empty() {
        let _ = std::fs::create_dir_all(&data_dir);
    }

    // Inherit PATH so sidecar can find tmux, git, etc.
    // macOS apps don't inherit shell PATH — must include homebrew paths for tmux/git
    let system_path = std::env::var("PATH").unwrap_or_default();
    let path_env = format!("/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:{}", system_path);
    let cmd = match app.shell().sidecar("tracker-server") {
        Ok(cmd) => cmd
            .env("TRACKER_RESOURCES_DIR", &resources_dir)
            .env("TRACKER_DATA_DIR", &data_dir)
            .env("PATH", &path_env),
        Err(e) => {
            eprintln!("Failed to create sidecar command: {e}");
            app.manage(SidecarState {
                child: Mutex::new(None),
                source: "offline",
            });
            return;
        }
    };

    eprintln!("Sidecar env: TRACKER_RESOURCES_DIR={resources_dir}");
    eprintln!("Sidecar env: TRACKER_DATA_DIR={data_dir}");

    match cmd.spawn() {
        Ok((_rx, child)) => {
            // Store PID for signal handler cleanup
            SIDECAR_PID.store(child.pid(), Ordering::Relaxed);
            // Wait for the server to become ready (poll health check)
            for i in 0..50 {
                if is_port_in_use(PORT) {
                    eprintln!("tracker-server sidecar ready after {i}x100ms");
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            app.manage(SidecarState {
                child: Mutex::new(Some(child)),
                source: "sidecar",
            });
        }
        Err(e) => {
            eprintln!("Failed to spawn sidecar: {e}");
            app.manage(SidecarState {
                child: Mutex::new(None),
                source: "offline",
            });
        }
    }
}

/// Kill the sidecar process if we spawned one.
fn stop_sidecar(app: &tauri::AppHandle) {
    SIDECAR_PID.store(0, Ordering::Relaxed);
    if let Some(state) = app.try_state::<SidecarState>() {
        if let Ok(mut guard) = state.child.lock() {
            if let Some(child) = guard.take() {
                let _ = child.kill();
                eprintln!("tracker-server sidecar killed");
            }
        }
    }
}

/// Apply vibrancy + make NSWindow non-opaque so rounded corners show through
#[cfg(target_os = "macos")]
fn apply_rounded_vibrancy(
    window: &tauri::WebviewWindow,
    material: NSVisualEffectMaterial,
    radius: f64,
) {
    apply_vibrancy(window, material, None, Some(radius)).ok();

    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return;
    };

    unsafe {
        use objc2::msg_send;
        use objc2::runtime::{AnyClass, AnyObject};

        let ns_view = appkit.ns_view.as_ptr() as *mut AnyObject;
        let ns_window: *mut AnyObject = msg_send![ns_view, window];

        // Transparent window background
        let _: () = msg_send![ns_window, setOpaque: false];
        let color_cls = AnyClass::get(c"NSColor").unwrap();
        let clear: *mut AnyObject = msg_send![color_cls, clearColor];
        let _: () = msg_send![ns_window, setBackgroundColor: clear];

        // Clip content view to rounded rectangle (masks WKWebView too)
        let content_view: *mut AnyObject = msg_send![ns_window, contentView];
        let _: () = msg_send![content_view, setWantsLayer: true];
        let layer: *mut AnyObject = msg_send![content_view, layer];
        let _: () = msg_send![layer, setCornerRadius: radius];
        let _: () = msg_send![layer, setMasksToBounds: true];
    }
}

#[tauri::command]
fn show_float(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("float") {
        let _ = win.show();
        let _ = win.set_focus();
    } else {
        let win = WebviewWindowBuilder::new(&app, "float", WebviewUrl::App("index.html".into()))
            .title("AgentTracker")
            .inner_size(340.0, 52.0)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .build()
            .map_err(|e: tauri::Error| e.to_string())?;
        #[cfg(target_os = "macos")]
        {
            apply_rounded_vibrancy(&win, NSVisualEffectMaterial::HudWindow, 8.0);
            // Make the float window stationary so double-click and Mission Control
            // don't push other windows around
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(handle) = win.window_handle() {
                if let RawWindowHandle::AppKit(appkit) = handle.as_raw() {
                    unsafe {
                        use objc2::msg_send;
                        use objc2::runtime::AnyObject;
                        let ns_view = appkit.ns_view.as_ptr() as *mut AnyObject;
                        let ns_window: *mut AnyObject = msg_send![ns_view, window];
                        // NSWindowCollectionBehaviorCanJoinAllSpaces (1)
                        // | NSWindowCollectionBehaviorStationary (16)
                        // | NSWindowCollectionBehaviorFullScreenAuxiliary (256)
                        // | NSWindowCollectionBehaviorIgnoresCycle (64)
                        let behavior: u64 = 1 | 16 | 64 | 256;
                        let _: () = msg_send![ns_window, setCollectionBehavior: behavior];

                        // Disable movable-by-background so macOS doesn't track
                        // title-bar double-click (zoom/minimize) on this window
                        let _: () = msg_send![ns_window, setMovableByWindowBackground: false];
                        let _: () = msg_send![ns_window, setMovable: false];

                        // Strip miniaturizable from style mask
                        let mask: u64 = msg_send![ns_window, styleMask];
                        let _: () = msg_send![ns_window, setStyleMask: mask & !4u64];
                    }
                }
            }
        }
        let _ = win.set_focus();
    }
    Ok(())
}

#[tauri::command]
fn hide_float(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("float") {
        let _ = win.hide();
    }
    Ok(())
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn restart_sidecar(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    const PORT: u16 = 3099;

    // 1. Kill current sidecar (only if we own it)
    let source = app
        .try_state::<SidecarState>()
        .map(|s| s.source)
        .unwrap_or("unknown");

    if source == "sidecar" {
        SIDECAR_PID.store(0, Ordering::Relaxed);
        if let Some(state) = app.try_state::<SidecarState>() {
            if let Ok(mut guard) = state.child.lock() {
                if let Some(child) = guard.take() {
                    let _ = child.kill();
                    eprintln!("restart_sidecar: killed old sidecar");
                }
            }
        }
    } else if source == "external" {
        return Ok(serde_json::json!({
            "success": false,
            "error": "Server is running externally (not managed by this app)"
        }));
    }

    // 2. Wait for port to be released (up to 5s)
    for _ in 0..50 {
        if !is_port_in_use(PORT) { break; }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    if is_port_in_use(PORT) {
        return Ok(serde_json::json!({
            "success": false,
            "error": "Port 3099 still in use after 5s"
        }));
    }

    // 3. Spawn new sidecar
    let resources_dir = app.path().resource_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let data_dir = app.path().app_data_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    // macOS apps don't inherit shell PATH — must include homebrew paths for tmux/git
    let system_path = std::env::var("PATH").unwrap_or_default();
    let path_env = format!("/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:{}", system_path);
    let cmd = app.shell().sidecar("tracker-server")
        .map_err(|e| format!("Failed to create sidecar command: {e}"))?
        .env("TRACKER_RESOURCES_DIR", &resources_dir)
        .env("TRACKER_DATA_DIR", &data_dir)
        .env("PATH", &path_env);

    let (_rx, child) = cmd.spawn()
        .map_err(|e| format!("Failed to spawn sidecar: {e}"))?;

    SIDECAR_PID.store(child.pid(), Ordering::Relaxed);

    // Update existing SidecarState
    if let Some(state) = app.try_state::<SidecarState>() {
        if let Ok(mut guard) = state.child.lock() {
            *guard = Some(child);
        }
    }

    // 4. Wait for health check (up to 10s)
    for i in 0..100 {
        if is_port_in_use(PORT) {
            eprintln!("restart_sidecar: new sidecar ready after {}ms", i * 100);
            return Ok(serde_json::json!({
                "success": true,
                "message": format!("Sidecar restarted in {}ms", i * 100)
            }));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    Ok(serde_json::json!({
        "success": false,
        "error": "Sidecar started but health check timed out after 10s"
    }))
}

/// Read auth token — try Application Support first, fall back to legacy path
#[tauri::command]
fn read_local_token(app: tauri::AppHandle) -> Result<String, String> {
    let paths: Vec<std::path::PathBuf> = [
        app.path().app_data_dir().ok().map(|p| p.join("agent-config.json")),
        Some(legacy_config_dir().join("agent-config.json")),
    ]
    .into_iter()
    .flatten()
    .collect();

    for path in &paths {
        if let Ok(data) = std::fs::read_to_string(path) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&data) {
                if let Some(token) = json["auth"]["token"].as_str().filter(|s| !s.is_empty()) {
                    return Ok(token.to_string());
                }
            }
        }
    }

    Err("No token found in any config location".to_string())
}

#[tauri::command]
fn open_dashboard(app: tauri::AppHandle) -> Result<(), String> {
    // If the dashboard window already exists, just focus it
    if let Some(win) = app.get_webview_window("dashboard") {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }

    // Build URL with auth token as query parameter for auto-authentication.
    // The web frontend reads ?token= from URL, stores it in localStorage, and cleans the URL.
    let url_str = match read_local_token(app.clone()) {
        Ok(token) => format!("http://localhost:3099?token={}", token.trim()),
        Err(_) => "http://localhost:3099".to_string(),
    };

    let url = WebviewUrl::External(url_str.parse().unwrap());
    let builder = WebviewWindowBuilder::new(&app, "dashboard", url)
        .title("AgentTracker Dashboard")
        .inner_size(1200.0, 800.0)
        .resizable(true);

    builder
        .build()
        .map_err(|e: tauri::Error| e.to_string())?;

    Ok(())
}

#[tauri::command]
fn get_server_status(app: tauri::AppHandle) -> serde_json::Value {
    let source = app
        .try_state::<SidecarState>()
        .map(|s| s.source)
        .unwrap_or("unknown");
    let running = is_port_in_use(3099);
    serde_json::json!({
        "source": source,
        "port": 3099,
        "running": running,
    })
}

#[tauri::command]
fn save_local_token(app: tauri::AppHandle, token: String) -> Result<(), String> {
    let path = app.path().app_data_dir()
        .map(|p| p.join("agent-config.json"))
        .map_err(|e| format!("Cannot resolve app data dir: {}", e))?;

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let mut json: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_else(|| serde_json::json!({"auth": {}}));

    json["auth"]["token"] = serde_json::Value::String(token);
    let output = serde_json::to_string_pretty(&json)
        .map_err(|e| e.to_string())?;
    std::fs::write(&path, output)
        .map_err(|e| format!("Cannot write config: {}", e))?;
    Ok(())
}

#[tauri::command]
fn set_float_opacity(app: tauri::AppHandle, opacity: f64) -> Result<(), String> {
    let opacity = opacity.clamp(0.1, 1.0);
    if let Some(win) = app.get_webview_window("float") {
        #[cfg(target_os = "macos")]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(handle) = win.window_handle() {
                if let RawWindowHandle::AppKit(appkit) = handle.as_raw() {
                    unsafe {
                        use objc2::msg_send;
                        use objc2::runtime::AnyObject;
                        let ns_view = appkit.ns_view.as_ptr() as *mut AnyObject;
                        let ns_window: *mut AnyObject = msg_send![ns_view, window];
                        let _: () = msg_send![ns_window, setAlphaValue: opacity];
                    }
                }
            }
        }
    }
    Ok(())
}

fn toggle_panel(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("panel") {
        if win.is_visible().unwrap_or(false) {
            let _ = win.hide();
        } else {
            use tauri_plugin_positioner::{Position, WindowExt};
            let _ = win.move_window(Position::TrayCenter);
            let _ = win.show();
            let _ = win.set_focus();
        }
    }
}

fn create_panel(app: &tauri::AppHandle) {
    let result = WebviewWindowBuilder::new(app, "panel", WebviewUrl::App("index.html".into()))
        .title("AgentTracker")
        .inner_size(280.0, 400.0)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .resizable(false)
        .build();

    if let Ok(panel) = result {
        #[cfg(target_os = "macos")]
        apply_rounded_vibrancy(&panel, NSVisualEffectMaterial::Popover, 10.0);

        let panel_clone = panel.as_ref().window().clone();
        panel.as_ref().window().on_window_event(move |event| {
            if let tauri::WindowEvent::Focused(false) = event {
                let _ = panel_clone.hide();
            }
        });
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Install early — before Tauri builder so we catch signals during init
    install_signal_handlers();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // If a second instance is launched, just focus the existing panel
            toggle_panel(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .invoke_handler(tauri::generate_handler![show_float, hide_float, set_float_opacity, open_dashboard, read_local_token, save_local_token, get_server_status, quit_app, restart_sidecar])
        .setup(|app| {
            // Enable autostart on first run
            use tauri_plugin_autostart::ManagerExt;
            let autostart = app.autolaunch();
            if !autostart.is_enabled().unwrap_or(false) {
                let _ = autostart.enable();
            }

            start_sidecar(app.handle());
            create_panel(app.handle());

            let tray_icon = {
                let icon_bytes = include_bytes!("../icons/tray-icon.png");
                tauri::image::Image::from_bytes(icon_bytes).expect("failed to load tray icon")
            };
            let mut tray_builder = TrayIconBuilder::new()
                .icon(tray_icon)
                .show_menu_on_left_click(false);
            #[cfg(target_os = "macos")]
            {
                tray_builder = tray_builder.icon_as_template(true);
            }
            let _tray = tray_builder
                .on_tray_icon_event(|tray, event| {
                    tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);

                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        toggle_panel(tray.app_handle());
                    }
                })
                .build(app)?;

            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Install signal handlers once, AFTER Tauri's own initialization,
            // so ours aren't overridden by the framework.
            static ONCE: std::sync::Once = std::sync::Once::new();
            ONCE.call_once(install_signal_handlers);

            if let RunEvent::ExitRequested { .. } = &event {
                stop_sidecar(app);
            }
        });
}
