use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::Context;
use console::{style, Term};
use cpal::traits::{DeviceTrait, HostTrait};
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};

use sonium_common::config::ClientConfig;
use sonium_control::discovery::{self, DiscoveredServer};

const DEFAULT_LINUX_PATH: &str = "/opt/sonium";

pub async fn run() -> anyhow::Result<()> {
    let term = Term::stdout();
    term.clear_screen()?;

    println!(
        "{}",
        style("=========================================")
            .cyan()
            .bold()
    );
    println!(
        "{}",
        style("        Sonium Client Installer          ")
            .cyan()
            .bold()
    );
    println!(
        "{}",
        style("=========================================")
            .cyan()
            .bold()
    );
    println!();

    #[cfg(target_os = "linux")]
    {
        // Elevate privileges on Linux
        if let Err(e) = sudo::escalate_if_needed() {
            eprintln!(
                "{}",
                style(format!(
                    "Warning: Could not escalate privileges automatically: {}",
                    e
                ))
                .yellow()
            );
            eprintln!("You may need to run this command with sudo.");
        }
    }

    let default_path = if cfg!(target_os = "macos") {
        dirs::home_dir()
            .map(|p| p.join(".sonium").to_string_lossy().to_string())
            .unwrap_or_else(|| "/usr/local/sonium".to_string())
    } else if cfg!(target_os = "windows") {
        dirs::data_local_dir()
            .map(|p| p.join("sonium").to_string_lossy().to_string())
            .unwrap_or_else(|| "C:\\ProgramData\\sonium".to_string())
    } else {
        DEFAULT_LINUX_PATH.to_string()
    };

    let install_dir_str: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Installation directory")
        .default(default_path)
        .interact_text()?;

    let install_dir = PathBuf::from(install_dir_str);
    if !install_dir.exists() {
        if Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(format!(
                "Directory {} does not exist. Create it?",
                install_dir.display()
            ))
            .default(true)
            .interact()?
        {
            fs::create_dir_all(&install_dir).context("Failed to create installation directory")?;
            println!("{}", style("✓ Directory created.").green());
        } else {
            anyhow::bail!("Installation cancelled.");
        }
    }

    // Attempt to discover server
    println!();
    println!(
        "{}",
        style("Looking for Sonium servers on the network...").dim()
    );
    let (server_host, server_port) = discover_or_manual().await?;

    println!();
    println!(
        "{}",
        style(format!("Using Server: {}:{}", server_host, server_port))
            .green()
            .bold()
    );
    println!();

    let mut instance_id = 1;

    // Main installation loop for multiple instances
    loop {
        println!(
            "{}",
            style(format!("--- Configuring Instance {} ---", instance_id)).cyan()
        );

        // Find used devices
        let used_devices = get_used_devices(&install_dir);

        // Select Device
        let host = cpal::default_host();
        let devices = host
            .output_devices()
            .context("Failed to get output devices")?;

        let mut device_names = Vec::new();
        for device in devices {
            if let Ok(name) = device.name() {
                if !device_names.contains(&name) {
                    device_names.push(name);
                }
            }
        }

        // Filter out used devices or mark them
        let mut menu_items = Vec::new();
        for name in &device_names {
            if used_devices.contains(name) {
                menu_items.push(format!("{} (Already in use)", name));
            } else {
                menu_items.push(name.clone());
            }
        }

        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt(format!("Select output device for instance {}", instance_id))
            .default(0)
            .items(&menu_items)
            .interact()?;

        let selected_device = &device_names[selection];
        println!("Selected device: {}", style(selected_device).green());

        // Copy binary
        let current_exe = env::current_exe()?;
        let target_exe = install_dir.join("sonium-client");
        fs::copy(&current_exe, &target_exe)
            .context("Failed to copy executable to installation directory")?;

        // Create Config
        let config_path = install_dir.join(format!("client-{}.toml", instance_id));
        let cfg = ClientConfig {
            server_host: server_host.clone(),
            server_port,
            device: Some(selected_device.clone()),
            instance: instance_id,
            ..Default::default()
        };

        let toml_string = toml::to_string_pretty(&cfg)?;
        fs::write(&config_path, toml_string).context("Failed to write config file")?;
        println!(
            "{} Config written to {}",
            style("✓").green(),
            config_path.display()
        );

        // Install Service
        install_service(&target_exe, &config_path, instance_id)?;

        println!();
        println!(
            "{}",
            style(format!("Instance {} installed successfully!", instance_id))
                .green()
                .bold()
        );
        println!();

        if !Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Do you want to configure another client instance on this machine?")
            .default(false)
            .interact()?
        {
            break;
        }

        instance_id += 1;
    }

    println!(
        "{}",
        style("Sonium Client installation complete!").cyan().bold()
    );
    Ok(())
}

fn get_used_devices(install_dir: &Path) -> Vec<String> {
    let mut used = Vec::new();
    if let Ok(entries) = fs::read_dir(install_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "toml") {
                if let Ok(contents) = fs::read_to_string(&path) {
                    if let Ok(cfg) = toml::from_str::<ClientConfig>(&contents) {
                        if let Some(device) = cfg.device {
                            used.push(device);
                        }
                    }
                }
            }
        }
    }
    used
}

async fn discover_or_manual() -> anyhow::Result<(String, u16)> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<DiscoveredServer>(32);
    tokio::spawn(discovery::browse_servers(tx));

    let mut servers: Vec<DiscoveredServer> = Vec::new();
    let deadline = tokio::time::sleep(Duration::from_secs(3));
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Some(s) => {
                        let key = format!("{}:{}", s.addr, s.port);
                        if !servers.iter().any(|x| format!("{}:{}", x.addr, x.port) == key) {
                            servers.push(s);
                        }
                    }
                    None => break,
                }
            }
            _ = &mut deadline => break,
        }
    }

    if servers.is_empty() {
        println!("{}", style("No servers found automatically.").yellow());
        return ask_manual_server();
    }

    let mut items = Vec::new();
    for s in &servers {
        items.push(format!("{} ({}:{})", s.hostname, s.addr, s.port));
    }
    items.push("Manual Entry".to_string());

    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select Sonium Server")
        .default(0)
        .items(&items)
        .interact()?;

    if selection == servers.len() {
        ask_manual_server()
    } else {
        let s = &servers[selection];
        Ok((s.addr.to_string(), s.port))
    }
}

fn ask_manual_server() -> anyhow::Result<(String, u16)> {
    let host: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Server IP or Hostname")
        .interact_text()?;

    let port: u16 = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Server Port")
        .default(1710)
        .interact_text()?;

    Ok((host, port))
}

#[cfg(target_os = "linux")]
fn install_service(exe_path: &Path, config_path: &Path, instance: u32) -> anyhow::Result<()> {
    let service_name = format!("sonium-client@{}.service", instance);
    let service_path = PathBuf::from("/etc/systemd/system").join(&service_name);

    let service_content = format!(
        r#"[Unit]
Description=Sonium Client (Instance {})
After=network-online.target sound.target
Wants=network-online.target

[Service]
Type=simple
ExecStart={} --config {}
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
"#,
        instance,
        exe_path.display(),
        config_path.display()
    );

    fs::write(&service_path, service_content).context("Failed to write systemd service file")?;
    println!(
        "{} Created systemd service at {}",
        style("✓").green(),
        service_path.display()
    );

    // Daemon reload
    let _ = Command::new("systemctl").arg("daemon-reload").output();

    // Enable and start
    let output = Command::new("systemctl")
        .arg("enable")
        .arg("--now")
        .arg(&service_name)
        .output()
        .context("Failed to execute systemctl")?;

    if output.status.success() {
        println!(
            "{} Started and enabled service {}",
            style("✓").green(),
            service_name
        );
    } else {
        eprintln!(
            "{}",
            style(format!(
                "Failed to start service: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
            .red()
        );
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn install_service(exe_path: &Path, config_path: &Path, instance: u32) -> anyhow::Result<()> {
    let plist_name = format!("com.sonium.client.{}.plist", instance);
    let mut launch_agents_dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    launch_agents_dir.push("Library");
    launch_agents_dir.push("LaunchAgents");

    if !launch_agents_dir.exists() {
        fs::create_dir_all(&launch_agents_dir)?;
    }

    let plist_path = launch_agents_dir.join(&plist_name);

    let plist_content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.sonium.client.{}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>--config</string>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#,
        instance,
        exe_path.display(),
        config_path.display()
    );

    fs::write(&plist_path, plist_content).context("Failed to write plist file")?;
    println!(
        "{} Created LaunchAgent at {}",
        style("✓").green(),
        plist_path.display()
    );

    // Unload first if it was already running to ensure clean state
    let _ = Command::new("launchctl")
        .arg("unload")
        .arg(&plist_path)
        .output();

    let output = Command::new("launchctl")
        .arg("load")
        .arg("-w")
        .arg(&plist_path)
        .output()
        .context("Failed to execute launchctl")?;

    if output.status.success() {
        println!(
            "{} Loaded and started LaunchAgent {}",
            style("✓").green(),
            plist_name
        );
    } else {
        eprintln!(
            "{}",
            style(format!(
                "Failed to load LaunchAgent: {}",
                String::from_utf8_lossy(&output.stderr)
            ))
            .red()
        );
    }

    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn install_service(_exe_path: &Path, _config_path: &Path, _instance: u32) -> anyhow::Result<()> {
    println!(
        "{}",
        style("Automatic service installation is currently only supported on Linux and macOS.")
            .yellow()
    );
    Ok(())
}

pub async fn uninstall() -> anyhow::Result<()> {
    println!(
        "{}",
        style("=========================================")
            .cyan()
            .bold()
    );
    println!(
        "{}",
        style("         Sonium Client Uninstaller       ")
            .cyan()
            .bold()
    );
    println!(
        "{}",
        style("=========================================")
            .cyan()
            .bold()
    );
    println!();

    #[cfg(target_os = "linux")]
    {
        if sudo::check() != sudo::RunningAs::Root {
            println!(
                "{}",
                style("Uninstalling services on Linux requires root privileges.").yellow()
            );
            let exe = std::env::current_exe()?;
            let status = std::process::Command::new("sudo")
                .arg(exe)
                .arg("--uninstall")
                .status()?;

            if !status.success() {
                anyhow::bail!("Failed to acquire root privileges.");
            }
            return Ok(());
        }

        // Stop and disable any sonium-client systemd services
        println!(
            "{} Stopping and disabling systemd services...",
            style("✔").green()
        );
        let _ = std::process::Command::new("systemctl")
            .arg("stop")
            .arg("sonium-client@*.service")
            .output();
        let _ = std::process::Command::new("systemctl")
            .arg("disable")
            .arg("sonium-client@*.service")
            .output();

        // Remove the unit file
        let systemd_dir = PathBuf::from("/etc/systemd/system");
        let unit_file = systemd_dir.join("sonium-client@.service");
        if unit_file.exists() {
            std::fs::remove_file(&unit_file)?;
            println!(
                "{} Removed systemd unit file at {:?}",
                style("✔").green(),
                unit_file
            );
        }

        let _ = std::process::Command::new("systemctl")
            .arg("daemon-reload")
            .output();
    }

    #[cfg(target_os = "macos")]
    {
        println!(
            "{} Stopping and removing LaunchAgents...",
            style("✔").green()
        );
        if let Some(home) = dirs::home_dir() {
            let launch_agents_dir = home.join("Library").join("LaunchAgents");
            if launch_agents_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(&launch_agents_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                            if name.starts_with("com.sonium.client.") && name.ends_with(".plist") {
                                let label = name.strip_suffix(".plist").unwrap_or(name);
                                
                                // Stop the service
                                let _ = std::process::Command::new("launchctl")
                                    .arg("stop")
                                    .arg(label)
                                    .output();

                                // Unload the service
                                let _ = std::process::Command::new("launchctl")
                                    .arg("unload")
                                    .arg(&path)
                                    .output();
                                    
                                // Remove from launchd by label (fallback)
                                let _ = std::process::Command::new("launchctl")
                                    .arg("remove")
                                    .arg(label)
                                    .output();

                                // Remove the plist file
                                let _ = std::fs::remove_file(&path);
                                println!("  {} Removed {}", style("✔").green(), name);
                            }
                        }
                    }
                }
            }
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        println!("{}", style("Automatic uninstallation of background services is only supported on Linux and macOS.").yellow());
    }

    // Ask to remove installation directory
    let default_path = if cfg!(target_os = "macos") {
        dirs::home_dir()
            .map(|p| p.join(".sonium").to_string_lossy().to_string())
            .unwrap_or_else(|| "/usr/local/sonium".to_string())
    } else if cfg!(target_os = "windows") {
        dirs::data_local_dir()
            .map(|p| p.join("sonium").to_string_lossy().to_string())
            .unwrap_or_else(|| "C:\\ProgramData\\sonium".to_string())
    } else {
        DEFAULT_LINUX_PATH.to_string()
    };

    let dir_to_remove_str: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter the installation directory to remove (leave empty to skip)")
        .default(default_path)
        .allow_empty(true)
        .interact_text()?;

    if !dir_to_remove_str.is_empty() {
        let dir_to_remove = PathBuf::from(&dir_to_remove_str);
        if dir_to_remove.exists() && dir_to_remove.is_dir() {
            let confirm = Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt(format!(
                    "Are you sure you want to completely delete {:?} and all its contents?",
                    dir_to_remove
                ))
                .default(false)
                .interact()?;

            if confirm {
                std::fs::remove_dir_all(&dir_to_remove)?;
                println!("{} Deleted installation directory.", style("✔").green());
            }
        } else {
            println!(
                "{} Directory does not exist or is not a directory.",
                style("⚠").yellow()
            );
        }
    }

    println!();
    println!("{}", style("Uninstallation complete.").green().bold());

    Ok(())
}
