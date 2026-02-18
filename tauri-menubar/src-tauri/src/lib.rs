use tauri::{
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WebviewUrl, WebviewWindowBuilder,
};
#[cfg(target_os = "macos")]
use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};

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
        apply_rounded_vibrancy(&win, NSVisualEffectMaterial::HudWindow, 8.0);
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
fn open_url(url: String) -> Result<(), String> {
    std::process::Command::new("open")
        .arg(&url)
        .spawn()
        .map_err(|e| e.to_string())?;
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
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .invoke_handler(tauri::generate_handler![show_float, hide_float, set_float_opacity, open_url, read_local_token])
        .setup(|app| {
            create_panel(app.handle());

            let tray_icon = {
                let icon_bytes = include_bytes!("../icons/tray-icon.png");
                tauri::image::Image::from_bytes(icon_bytes).expect("failed to load tray icon")
            };
            let _tray = TrayIconBuilder::new()
                .icon(tray_icon)
                .icon_as_template(true)
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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
