mod asr;
mod audio;
mod config;
mod error;
mod flow;
mod inject;
mod keychain;
mod state;
mod tray;

use std::time::Duration;

use tauri::menu::MenuEvent;
use tauri::tray::TrayIconEvent;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::config::{AppConfig, ConfigPatch};
use crate::error::{ClientError, ErrorKind};
use crate::flow::{AsrStatus, HealthWire};
use crate::state::ConfigHolder;

const PANEL_LABEL: &str = "panel";
const HEALTH_INTERVAL: Duration = Duration::from_secs(5);

/// Poll /healthz from Rust and publish changes as `asr://status` updates.
fn spawn_health_monitor(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let base_url = app.state::<ConfigHolder>().get().base_url;
            let health = if asr::healthz(&base_url).await.is_ok() {
                HealthWire::Online
            } else {
                HealthWire::Offline
            };
            if app.state::<flow::Session>().snapshot().health != health {
                flow::set_health(&app, health);
            }
            tokio::time::sleep(HEALTH_INTERVAL).await;
        }
    });
}

fn register_hotkey(app: &AppHandle, hotkey: &str) -> Result<(), ClientError> {
    app.global_shortcut()
        .on_shortcut(hotkey, |app, _shortcut, event| {
            // Toggle on press only; release events would double-fire.
            if event.state == ShortcutState::Pressed {
                flow::toggle(app);
            }
        })
        .map_err(|e| ClientError::Mapped(
            ErrorKind::Hotkey,
            format!("could not register hotkey \"{hotkey}\": {e}"),
        ))
}

fn unregister_hotkey(app: &AppHandle, hotkey: &str) {
    let _ = app.global_shortcut().unregister(hotkey);
}

fn show_panel(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(win) = app.get_webview_window(PANEL_LABEL) {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

fn hide_panel(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(PANEL_LABEL) {
        let _ = win.hide();
    }
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
}

// ---------------------------------------------------------------- commands

#[tauri::command]
fn get_status(app: AppHandle) -> AsrStatus {
    flow::current_status(&app)
}

#[tauri::command]
fn start_recording(app: AppHandle) -> Result<audio::StartInfo, ClientError> {
    flow::start_recording(&app)
}

#[tauri::command]
fn stop_recording(app: AppHandle) -> Result<(), ClientError> {
    if app.state::<flow::Session>().snapshot().phase != flow::PhaseWire::Recording {
        return Err(ClientError::audio("not recording"));
    }
    flow::stop_and_transcribe(&app);
    Ok(())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigView {
    #[serde(flatten)]
    config: AppConfig,
    /// Keychain presence only — the token value never crosses into JS.
    has_token: bool,
}

#[tauri::command]
fn get_config(app: AppHandle) -> ConfigView {
    ConfigView {
        config: app.state::<ConfigHolder>().get(),
        has_token: keychain::has_token(),
    }
}

#[tauri::command]
fn set_config(app: AppHandle, patch: ConfigPatch) -> Result<ConfigView, ClientError> {
    let holder = app.state::<ConfigHolder>();
    let current = holder.get();
    let mut next = current.clone();
    if let Some(url) = patch.base_url.filter(|u| !u.trim().is_empty()) {
        next.base_url = url.trim_end_matches('/').to_string();
    }
    if let Some(hotkey) = patch.hotkey.filter(|h| !h.trim().is_empty()) {
        next.hotkey = hotkey.trim().to_string();
    }
    if let Some(cleanup) = patch.cleanup {
        next.cleanup = cleanup;
    }

    // Validate the accelerator before persisting: bad accelerators must not stick.
    let hotkey_changed = next.hotkey != current.hotkey;
    if hotkey_changed {
        tauri_plugin_global_shortcut::Shortcut::try_from(next.hotkey.as_str()).map_err(|_| {
            ClientError::Mapped(
                ErrorKind::Hotkey,
                format!("invalid hotkey \"{}\"", next.hotkey),
            )
        })?;
    }

    next.save(&holder.path)?;
    holder.set(next.clone());

    if hotkey_changed {
        unregister_hotkey(&app, &current.hotkey);
        register_hotkey(&app, &next.hotkey)?;
    }
    Ok(ConfigView {
        config: next,
        has_token: keychain::has_token(),
    })
}

#[tauri::command]
fn save_token(app: AppHandle, token: String) -> Result<ConfigView, ClientError> {
    let token = token.trim().to_string();
    if token.is_empty() {
        keychain::delete_token()?;
    } else {
        keychain::save_token(&token)?;
    }
    Ok(ConfigView {
        config: app.state::<ConfigHolder>().get(),
        has_token: keychain::has_token(),
    })
}

#[tauri::command]
fn delete_token(app: AppHandle) -> Result<ConfigView, ClientError> {
    keychain::delete_token()?;
    Ok(ConfigView {
        config: app.state::<ConfigHolder>().get(),
        has_token: false,
    })
}

#[tauri::command]
fn open_panel(app: AppHandle) {
    show_panel(&app);
}

#[tauri::command]
fn hide_window(app: AppHandle) {
    hide_panel(&app);
}

/// Accessibility permission is required to synthesize ⌘V. macOS won't prompt
/// on its own for posted events, so expose an explicit check (+ optional
/// system prompt) and a jump-to-settings.
#[tauri::command]
fn accessibility_status(prompt: Option<bool>) -> bool {
    inject::is_trusted_for_accessibility(prompt.unwrap_or(false))
}

#[tauri::command]
fn open_accessibility_settings() {
    inject::open_accessibility_pane();
}

// ---------------------------------------------------------------- wiring

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let handle = app.handle().clone();

            let config_path = handle
                .path()
                .app_config_dir()
                .map(|d| d.join("config.json"))
                .unwrap_or_else(|_| std::path::PathBuf::from("asr-config.json"));
            app.manage(ConfigHolder::new(config_path));
            app.manage(flow::Session::default());

            tray::build_tray(&handle)?;

            // Menu-bar app: no Dock icon, panel window created hidden.
            #[cfg(target_os = "macos")]
            handle.set_activation_policy(tauri::ActivationPolicy::Accessory)?;

            let _panel = WebviewWindowBuilder::new(
                &handle,
                PANEL_LABEL,
                WebviewUrl::App("index.html".into()),
            )
            .title("ASR")
            .inner_size(380.0, 560.0)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .skip_taskbar(true)
            .visible(false)
            .center()
            .build()?;
            // Hotkey from config; a conflict is surfaced, not fatal.
            let hotkey = handle.state::<ConfigHolder>().get().hotkey;
            if let Err(e) = register_hotkey(&handle, &hotkey) {
                eprintln!("[asr] {e}");
                flow::report_error(&handle, e);
            }

            spawn_health_monitor(handle.clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == PANEL_LABEL {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    // Hide instead of destroy; the app lives in the menu bar.
                    api.prevent_close();
                    hide_panel(&window.app_handle().clone());
                }
            }
        })
        .on_menu_event(|app: &AppHandle, event: MenuEvent| match event.id().as_ref() {
            tray::ID_TOGGLE => flow::toggle(app),
            tray::ID_PANEL => show_panel(app),
            tray::ID_QUIT => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { .. } = event {
                let app = tray.app_handle();
                if let Some(win) = app.get_webview_window(PANEL_LABEL) {
                    if win.is_visible().unwrap_or(false) {
                        hide_panel(app);
                    } else {
                        show_panel(app);
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            start_recording,
            stop_recording,
            get_config,
            set_config,
            save_token,
            delete_token,
            open_panel,
            hide_window,
            accessibility_status,
            open_accessibility_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
