use cpal::traits::{DeviceTrait, HostTrait};
use sonium_client_lib::controller;
use sonium_common::config::ClientConfig;
use sonium_protocol::messages::HealthReport;
use std::collections::HashMap;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};
use tokio::sync::{mpsc, Mutex};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InstanceConfig {
    pub id: u32,
    pub name: String,
    pub server_host: String,
    pub server_port: u16,
    pub device: Option<String>,
    pub latency_ms: i32,
    pub enabled: bool,
}

// Global state to track running tasks
struct AppState {
    running_instances: Arc<Mutex<HashMap<u32, tauri::async_runtime::JoinHandle<()>>>>,
}

fn config_path(app_handle: &tauri::AppHandle) -> std::path::PathBuf {
    let dir = app_handle
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    if !dir.exists() {
        let _ = std::fs::create_dir_all(&dir);
    }
    dir.join("instances.json")
}

#[tauri::command]
async fn scan_subnet() -> Result<Vec<String>, String> {
    let local_ip = local_ip_address::local_ip().map_err(|e| e.to_string())?;
    if !local_ip.is_ipv4() {
        return Err("Only IPv4 subnet scanning is supported".into());
    }
    let ip_str = local_ip.to_string();
    let parts: Vec<&str> = ip_str.split('.').collect();
    if parts.len() != 4 {
        return Err("Invalid local IP".into());
    }
    let base = format!("{}.{}.{}", parts[0], parts[1], parts[2]);

    let mut tasks = Vec::new();
    for i in 1..255 {
        let target_ip = format!("{}.{}", base, i);
        tasks.push(tokio::spawn(async move {
            let addr = format!("{}:1710", target_ip);
            // 500ms timeout for scanning
            match tokio::time::timeout(
                tokio::time::Duration::from_millis(500),
                tokio::net::TcpStream::connect(&addr),
            )
            .await
            {
                Ok(Ok(_)) => Some(target_ip),
                _ => None,
            }
        }));
    }

    let mut found = Vec::new();
    for task in tasks {
        if let Ok(Some(ip)) = task.await {
            found.push(ip);
        }
    }
    Ok(found)
}

#[tauri::command]
async fn get_local_ip() -> Result<String, String> {
    local_ip_address::local_ip()
        .map(|ip| ip.to_string())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_instances(app: tauri::AppHandle) -> Result<Vec<InstanceConfig>, String> {
    let path = config_path(&app);
    if path.exists() {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let instances = serde_json::from_str(&content).unwrap_or_default();
        Ok(instances)
    } else {
        Ok(Vec::new())
    }
}

#[tauri::command]
async fn save_instances(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    instances: Vec<InstanceConfig>,
) -> Result<(), String> {
    let path = config_path(&app);
    let content = serde_json::to_string_pretty(&instances).map_err(|e| e.to_string())?;
    std::fs::write(path, content).map_err(|e| e.to_string())?;

    // Sync running state
    let mut running = state.running_instances.lock().await;

    // First, stop any instances that are deleted or disabled
    let current_ids: Vec<u32> = instances
        .iter()
        .filter(|i| i.enabled)
        .map(|i| i.id)
        .collect();
    let mut to_remove = Vec::new();
    for id in running.keys() {
        if !current_ids.contains(id) {
            to_remove.push(*id);
        }
    }
    for id in to_remove {
        if let Some(handle) = running.remove(&id) {
            handle.abort();
        }
    }

    // Then, start or restart any enabled instances
    // For simplicity we just restart them all to apply new config
    for config in instances {
        if config.enabled {
            if let Some(handle) = running.remove(&config.id) {
                handle.abort();
            }

            let cfg = ClientConfig {
                server_host: config.server_host.clone(),
                server_port: config.server_port,
                device: config.device.clone(),
                instance: config.id,
                client_name: Some(config.name.clone()),
                latency_ms: config.latency_ms,
                ..Default::default()
            };

            let server_addr = format!("{}:{}", cfg.server_host, cfg.server_port);
            let (health_tx, mut health_rx) = mpsc::unbounded_channel::<HealthReport>();
            let app_handle = app.clone();
            let instance_id = config.id;

            let handle = tauri::async_runtime::spawn(async move {
                let _ = controller::run(server_addr, cfg, Some(health_tx)).await;
            });

            // Monitor health for this instance
            tauri::async_runtime::spawn(async move {
                while let Some(report) = health_rx.recv().await {
                    let _ = app_handle.emit(&format!("health:{}", instance_id), report);
                }
            });

            running.insert(config.id, handle);
        }
    }

    Ok(())
}

#[tauri::command]
async fn get_audio_devices() -> Result<Vec<String>, String> {
    let host = cpal::default_host();
    let devices = host.output_devices().map_err(|e| e.to_string())?;

    let mut names = Vec::new();
    for dev in devices {
        if let Ok(name) = dev.name() {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    Ok(names)
}

#[tauri::command]
async fn get_default_audio_device() -> Result<String, String> {
    let host = cpal::default_host();
    let dev = host.default_output_device().ok_or("No default device")?;
    dev.name().map_err(|e| e.to_string())
}

#[tauri::command]
async fn start_instance(
    state: tauri::State<'_, AppState>,
    app: tauri::AppHandle,
    config: InstanceConfig,
) -> Result<(), String> {
    let mut instances = state.running_instances.lock().await;

    // Stop if already running
    if let Some(handle) = instances.remove(&config.id) {
        handle.abort();
    }

    if !config.enabled {
        return Ok(());
    }

    let cfg = ClientConfig {
        server_host: config.server_host.clone(),
        server_port: config.server_port,
        device: config.device,
        instance: config.id,
        client_name: Some(config.name),
        latency_ms: config.latency_ms,
        ..Default::default()
    };

    let server_addr = format!("{}:{}", cfg.server_host, cfg.server_port);
    let (health_tx, mut health_rx) = mpsc::unbounded_channel::<HealthReport>();
    let app_handle = app.clone();
    let instance_id = config.id;

    let handle = tauri::async_runtime::spawn(async move {
        let _ = controller::run(server_addr, cfg, Some(health_tx)).await;
    });

    // Monitor health for this instance
    tauri::async_runtime::spawn(async move {
        while let Some(report) = health_rx.recv().await {
            let _ = app_handle.emit(&format!("health:{}", instance_id), report);
        }
    });

    instances.insert(config.id, handle);
    Ok(())
}

#[tauri::command]
async fn stop_instance(state: tauri::State<'_, AppState>, id: u32) -> Result<(), String> {
    let mut instances = state.running_instances.lock().await;
    if let Some(handle) = instances.remove(&id) {
        handle.abort();
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            running_instances: Arc::new(Mutex::new(HashMap::new())),
        })
        .invoke_handler(tauri::generate_handler![
            get_instances,
            save_instances,
            get_audio_devices,
            get_default_audio_device,
            start_instance,
            stop_instance,
            get_local_ip,
            scan_subnet
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Regular);

            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Ok(instances) = get_instances(app_handle.clone()).await {
                    let state = app_handle.state::<AppState>();
                    let _ = save_instances(state, app_handle.clone(), instances).await;
                }
            });

            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let settings_i = MenuItem::with_id(app, "settings", "Settings...", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[&settings_i, &PredefinedMenuItem::separator(app)?, &quit_i],
            )?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().unwrap_or_else(|| {
                    // Fallback to a blank icon or handle error gracefully
                    tauri::image::Image::new(&[], 0, 0)
                }))
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "settings" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        if let Some(window) = tray.app_handle().get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                window.hide().unwrap();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
