use std::sync::Mutex;

use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, RunEvent, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::CommandChild;
#[cfg(target_os = "macos")]
use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};

/// Holds the sidecar child process so we can kill it on exit.
struct SidecarState {
    child: Mutex<Option<CommandChild>>,
    /// "sidecar" if we spawned it, "external" if port was already occupied
    source: &'static str,
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

    let cmd = match app.shell().sidecar("tracker-server") {
        Ok(cmd) => cmd,
        Err(e) => {
            eprintln!("Failed to create sidecar command: {e}");
            app.manage(SidecarState {
                child: Mutex::new(None),
                source: "offline",
            });
            return;
        }
    };

    match cmd.spawn() {
        Ok((_rx, child)) => {
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
            .title("Agent Tracker")
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

/// Read auth token from local agent-config.json
#[tauri::command]
fn read_local_token() -> Result<String, String> {
    let home = std::env::var("HOME").map_err(|e| e.to_string())?;
    let path = std::path::PathBuf::from(home)
        .join(".config/agent-tracker/agent-config.json");
    let data = std::fs::read_to_string(&path)
        .map_err(|e| format!("Cannot read config: {}", e))?;
    let json: serde_json::Value = serde_json::from_str(&data)
        .map_err(|e| format!("Invalid JSON: {}", e))?;
    json["auth"]["token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| "No token in config".to_string())
}

#[tauri::command]
fn open_dashboard(app: tauri::AppHandle) -> Result<(), String> {
    // If the dashboard window already exists, just focus it
    if let Some(win) = app.get_webview_window("dashboard") {
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }

    // Read auth token to auto-authenticate the dashboard
    let token_script = match read_local_token() {
        Ok(token) => {
            // Escape backslashes and quotes for JS string literal
            let escaped = token.replace('\\', "\\\\").replace('\'', "\\'");
            format!(
                "localStorage.setItem('agent-tracker-auth-token', '{}');",
                escaped
            )
        }
        Err(_) => String::new(),
    };

    // Create a new dashboard window pointing at the tracker-server web UI
    let url = WebviewUrl::External("http://localhost:3099".parse().unwrap());
    let mut builder = WebviewWindowBuilder::new(&app, "dashboard", url)
        .title("Agent Tracker Dashboard")
        .inner_size(1200.0, 800.0)
        .resizable(true);

    if !token_script.is_empty() {
        builder = builder.initialization_script(&token_script);
    }

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
fn save_local_token(token: String) -> Result<(), String> {
    let home = std::env::var("HOME").map_err(|e| e.to_string())?;
    let path = std::path::PathBuf::from(home)
        .join(".config/agent-tracker/agent-config.json");
    let data = std::fs::read_to_string(&path)
        .map_err(|e| format!("Cannot read config: {}", e))?;
    let mut json: serde_json::Value = serde_json::from_str(&data)
        .map_err(|e| format!("Invalid JSON: {}", e))?;
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
        .title("Agent Tracker")
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
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .invoke_handler(tauri::generate_handler![show_float, hide_float, set_float_opacity, open_dashboard, read_local_token, save_local_token, get_server_status, quit_app])
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
            let quit_item = MenuItemBuilder::with_id("quit", "Quit Agent Tracker")
                .build(app)?;
            let tray_menu = MenuBuilder::new(app)
                .item(&quit_item)
                .build()?;

            let _tray = TrayIconBuilder::new()
                .icon(tray_icon)
                .icon_as_template(true)
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    if event.id() == "quit" {
                        app.exit(0);
                    }
                })
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
            if let RunEvent::ExitRequested { .. } = &event {
                stop_sidecar(app);
            }
        });
}
