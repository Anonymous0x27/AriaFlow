//! AriaFlow desktop shell (Tauri 2).
//!
//! The shell hosts the Vue frontend. All download business logic lives in the
//! AriaFlow daemon (`ariaflowd`); the frontend talks to it over the local HTTP
//! API (default `http://127.0.0.1:8787`). Closing this window never stops
//! background downloads — and, with the tray enabled, does not even quit the
//! UI process: the window hides and AriaFlow keeps running in the system tray
//! (Windows notification area / macOS menu bar / Linux system tray).
//!
//! Quit path: the tray menu's "退出 AriaFlow" emits `ariaflow-quit` to the
//! frontend (which reads the daemon global setting `autoQuitDaemon`, stored
//! server-side). When enabled, the frontend asks us to shut ariaflowd down
//! gracefully (`ariaflow_quit_daemon`) before exiting; otherwise ariaflowd
//! keeps running. A watchdog forces the app out after 5s no matter what.
//!
//! Planned milestone: embed `ariaflowd` as a Tauri sidecar so a fresh install
//! works without a separate daemon process (tracked in docs/BUILD.md).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ipc;
mod quit;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, WindowEvent};

/// One HTTP request/response over the local ariaflowd pipe (see ipc.rs).
#[tauri::command]
async fn ariaflow_request(
    method: String,
    path: String,
    body: Option<serde_json::Value>,
    token: Option<String>,
    ipc_path: Option<String>,
) -> Result<ipc::IpcResponse, ipc::TransportError> {
    let endpoint = ipc::resolve_endpoint(ipc_path.as_deref());
    let body_bytes = match &body {
        Some(v) => Some(serde_json::to_vec(v).map_err(ipc::TransportError::protocol)?),
        None => None,
    };
    ipc::ipc_roundtrip(&method, &path, body_bytes.as_deref(), token.as_deref(), &endpoint).await
}

/// Read-only probe for the settings page (never spawns anything).
#[tauri::command]
async fn ariaflow_daemon_status(
    ipc_path: Option<String>,
    webapi_base: Option<String>,
) -> Result<ipc::DaemonCheck, String> {
    Ok(ipc::daemon_status(ipc_path.as_deref(), webapi_base.as_deref()).await)
}

/// Probe + auto-start. Called once at app launch (when transport != webapi)
/// and from the Settings page "Start daemon" button.
#[tauri::command]
async fn ariaflow_ensure_daemon(
    binary_path: Option<String>,
    ipc_path: Option<String>,
    token: Option<String>,
    webapi_base: Option<String>,
) -> Result<ipc::DaemonCheck, String> {
    Ok(
        ipc::ensure_daemon(
            binary_path.as_deref(),
            ipc_path.as_deref(),
            token.as_deref(),
            webapi_base.as_deref(),
        )
        .await,
    )
}

/// Locate an ariaflowd binary (PATH + common install locations).
#[tauri::command]
fn ariaflow_find_daemon() -> Option<String> {
    ipc::find_ariaflowd()
}

/// Shut ariaflowd down (used by the "auto quit ariaflowd" option).
/// `wait` (default true): block up to 3s for the daemon to exit. The tray
/// "退出" path passes `wait: false` so the UI exits instantly and ariaflowd
/// shuts itself down in the background. Best-effort either way; the frontend
/// exits regardless of the outcome.
#[tauri::command]
async fn ariaflow_quit_daemon(wait: Option<bool>) -> Result<String, String> {
    if wait == Some(false) {
        quit::signal_daemon_stop()
    } else {
        quit::quit_daemon().await
    }
}

/// Exit the desktop app. Called by the frontend after it handled `ariaflow-quit`.
#[tauri::command]
fn ariaflow_exit(app: tauri::AppHandle) {
    app.exit(0);
}

/// Tray "退出 AriaFlow": hand the quit decision to the frontend (it owns the
/// API token), and force the app out after 5s no matter what happens there.
fn request_quit(app: &tauri::AppHandle) {
    let _ = app.emit("ariaflow-quit", ());
    let watchdog = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(5));
        watchdog.exit(0);
    });
}

fn main() {
    // Linux + WebKitGTK: on some driver/compositor stacks (notably NVIDIA
    // proprietary and several Mesa/Wayland combinations) the window renders
    // completely blank unless the DMA-BUF accelerated compositing path is
    // disabled. Disable it by default on Linux before GTK/WebKit initializes.
    //
    // Users can force-enable GPU compositing by setting
    // ARIAFLOW_WEBKIT_COMPOSITING=1, or control the WebKitGTK behaviour
    // themselves via WEBKIT_DISABLE_COMPOSITING_MODE (their value wins).
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("WEBKIT_DISABLE_COMPOSITING_MODE").is_none()
            && std::env::var_os("ARIAFLOW_WEBKIT_COMPOSITING").is_none()
        {
            std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
            eprintln!(
                "ariaflow-desktop: Linux/WebKitGTK — setting WEBKIT_DISABLE_COMPOSITING_MODE=1 \
                 (blank-window workaround; set ARIAFLOW_WEBKIT_COMPOSITING=1 to force GPU compositing)"
            );
        }

        // WebKitGTK ≥ 2.42 的 DMA-BUF 渲染路径在部分 NVIDIA/Mesa 驱动上
        // 直接崩溃或黑屏，必须用 WEBKIT_DISABLE_DMABUF_RENDERER=1 关掉。
        // 规则与上面一致：用户显式设置的 WEBKIT_DISABLE_DMABUF_RENDERER 永远优先；
        // ARIAFLOW_WEBKIT_DMABUF=1 可关闭本兜底（启用 DMA-BUF 渲染器）。
        if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none()
            && std::env::var_os("ARIAFLOW_WEBKIT_DMABUF").is_none()
        {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
            eprintln!(
                "ariaflow-desktop: Linux/WebKitGTK — setting WEBKIT_DISABLE_DMABUF_RENDERER=1 \
                 (DMA-BUF renderer workaround; set ARIAFLOW_WEBKIT_DMABUF=1 to force-enable it)"
            );
        }
    }

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            ariaflow_request,
            ariaflow_daemon_status,
            ariaflow_ensure_daemon,
            ariaflow_find_daemon,
            ariaflow_quit_daemon,
            ariaflow_exit
        ])
        .setup(|app| {
            // Ensure the window keeps the product branding on every OS.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title("AriaFlow");
            }

            // ---- System tray: AriaFlow stays resident until explicitly quit ----
            let app_handle = app.handle().clone();

            let show_item = MenuItem::with_id(
                app,
                "tray-show",
                "显示 AriaFlow",
                true,
                None::<&str>,
            )?;
            let quit_item = MenuItem::with_id(
                app,
                "tray-quit",
                "退出 AriaFlow",
                true,
                None::<&str>,
            )?;
            let separator = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(app, &[&show_item, &separator, &quit_item])?;

            let tray_icon = app
                .default_window_icon()
                .expect("tauri.conf.json must reference a window icon")
                .clone();

            TrayIconBuilder::with_id("ariaflow-tray")
                .icon(tray_icon)
                .menu(&menu)
                .tooltip("AriaFlow — 下载管理器（后台常驻）")
                // Left-click toggles the window; right-click opens the menu.
                .show_menu_on_left_click(false)
                .on_menu_event({
                    let app_handle = app_handle.clone();
                    move |_app, event| match event.id().as_ref() {
                        "tray-show" => show_main_window(&app_handle),
                        "tray-quit" => request_quit(&app_handle),
                        _ => {}
                    }
                })
                .on_tray_icon_event(move |tray, event| {
                    let toggled = matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        }
                    );
                    if toggled {
                        let handle = tray.app_handle();
                        if let Some(window) = handle.get_webview_window("main") {
                            let visible = window.is_visible().unwrap_or(false);
                            if visible {
                                let _ = window.hide();
                            } else {
                                show_main_window(handle);
                            }
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window hides it instead of quitting; use the tray
            // menu's "退出 AriaFlow" to actually exit.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running the AriaFlow desktop application");
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
