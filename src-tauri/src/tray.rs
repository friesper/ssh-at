use tauri::{
    menu::{IsMenuItem, Menu, MenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, Runtime, AppHandle,
};

#[cfg(target_os = "macos")]
use objc2_app_kit::NSApplication;
#[cfg(target_os = "macos")]
use objc2_foundation::MainThreadMarker;

use crate::commands::system::open_ssh_session;

const TRAY_ID: &str = "main";
const HOST_ITEM_PREFIX: &str = "host:";

/// 构建托盘菜单：Show / Hide / Quick Open 子菜单 / Quit
fn build_menu<R: Runtime>(app: &AppHandle<R>, hosts: &[(String, String)]) -> tauri::Result<Menu<R>> {
    let show_i = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
    let hide_i = MenuItem::with_id(app, "hide", "Hide", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    // (ssh 别名, 展示名)：中文别名只影响菜单文案，id 仍用 ASCII 别名
    let host_items = hosts
        .iter()
        .map(|(host, label)| {
            MenuItem::with_id(
                app,
                format!("{}{}", HOST_ITEM_PREFIX, host),
                label,
                true,
                None::<&str>,
            )
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    let host_refs: Vec<&dyn IsMenuItem<R>> = host_items
        .iter()
        .map(|item| item as &dyn IsMenuItem<R>)
        .collect();

    // 没有主机时禁用子菜单，避免出现空的弹出项
    let quick_open = Submenu::with_items(app, "Quick Open", !host_refs.is_empty(), &host_refs)?;

    Menu::with_items(app, &[&show_i, &hide_i, &quick_open, &quit_i])
}

/// 用最新的主机列表重建托盘菜单（前端每次加载配置后调用）。
/// macOS 的 Menu/MenuItem 必须在主线程创建与挂载，
/// 因此这里统一调度到主线程执行，避免在 async 命令线程里静默失效
pub fn refresh_hosts<R: Runtime>(app: &AppHandle<R>, hosts: Vec<(String, String)>) {
    let app_handle = app.clone();

    eprintln!("[TRAY] Refresh menu with {} hosts", hosts.len());

    let scheduled = app.run_on_main_thread(move || {
        match build_menu(&app_handle, &hosts) {
            Ok(menu) => {
                if let Some(tray) = app_handle.tray_by_id(TRAY_ID) {
                    if let Err(e) = tray.set_menu(Some(menu)) {
                        eprintln!("[TRAY] Failed to refresh menu: {}", e);
                    }
                } else {
                    eprintln!("[TRAY] Tray icon not found: {}", TRAY_ID);
                }
            }
            Err(e) => eprintln!("[TRAY] Failed to build menu: {}", e),
        }
    });

    if let Err(e) = scheduled {
        eprintln!("[TRAY] Failed to schedule menu refresh: {}", e);
    }
}

pub fn create_tray<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let menu = build_menu(app, &[])?;

    // Load and decode icon from embedded bytes - use icon.png for consistency with Dock icon
    let icon_bytes = include_bytes!("../icons/icon.png");
    let img = image::load_from_memory(icon_bytes)
        .map_err(|e| tauri::Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    let raw_pixels = rgba.into_raw();
    let icon = tauri::image::Image::new(&raw_pixels, width, height);

    let _tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .menu(&menu)
        .on_menu_event(move |app, event| {
            let id = event.id.as_ref();

            // 快速打开：菜单项 id 形如 host:<主机别名>
            if let Some(host) = id.strip_prefix(HOST_ITEM_PREFIX) {
                eprintln!("[TRAY] Quick open host: {}", host);
                if let Err(e) = open_ssh_session(host) {
                    eprintln!("[TRAY] Quick open failed: {}", e);
                }
                return;
            }

            match id {
                "quit" => {
                    eprintln!("[TRAY] Quit menu item clicked");
                    app.exit(0);
                }
                "show" => {
                    eprintln!("[TRAY] Show menu item clicked");

                    #[cfg(target_os = "macos")]
                    {
                        let mtm = unsafe { MainThreadMarker::new_unchecked() };
                        let app = NSApplication::sharedApplication(mtm);
                        app.setActivationPolicy(objc2_app_kit::NSApplicationActivationPolicy::Regular);
                    }

                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                "hide" => {
                    eprintln!("[TRAY] Hide menu item clicked");

                    #[cfg(target_os = "macos")]
                    {
                        let mtm = unsafe { MainThreadMarker::new_unchecked() };
                        let app = NSApplication::sharedApplication(mtm);
                        app.setActivationPolicy(objc2_app_kit::NSApplicationActivationPolicy::Accessory);
                    }

                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.hide();
                    }
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                eprintln!("[TRAY] Tray icon clicked");
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    if window.is_visible().unwrap_or(false) {
                        #[cfg(target_os = "macos")]
                        {
                            let mtm = unsafe { MainThreadMarker::new_unchecked() };
                            let ns_app = NSApplication::sharedApplication(mtm);
                            ns_app.setActivationPolicy(objc2_app_kit::NSApplicationActivationPolicy::Accessory);
                        }

                        let _ = window.hide();
                    } else {
                        #[cfg(target_os = "macos")]
                        {
                            let mtm = unsafe { MainThreadMarker::new_unchecked() };
                            let ns_app = NSApplication::sharedApplication(mtm);
                            ns_app.setActivationPolicy(objc2_app_kit::NSApplicationActivationPolicy::Regular);
                        }

                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
            }
        })
        .build(app)?;

    Ok(())
}
