//! System information, dependency status, and lightweight log access.

use std::path::Path;
use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, Request, State},
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use serde::Serialize;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

use crate::auth::{Claims, UserStore};
use crate::auth_api::AuthUser;

pub fn router(store: Arc<UserStore>) -> Router {
    Router::new()
        .route("/system/info", get(get_system_info))
        .route("/system/logs", get(get_logs))
        .route("/system/restart", post(post_restart))
        .route(
            "/system/dependencies/:id/:action",
            post(post_dependency_action),
        )
        .layer(middleware::from_fn_with_state(
            store.clone(),
            require_admin_auth,
        ))
        .with_state(store)
}

async fn require_admin_auth(
    State(store): State<Arc<UserStore>>,
    mut req: Request,
    next: Next,
) -> Response {
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));

    let claims: Option<Claims> = token.and_then(|t| store.verify_token(t));
    match claims {
        Some(c) if c.role == "admin" => {
            req.extensions_mut().insert(AuthUser(c));
            next.run(req).await
        }
        Some(_) => (StatusCode::FORBIDDEN, "admin role required").into_response(),
        None => (StatusCode::UNAUTHORIZED, "missing or invalid token").into_response(),
    }
}

#[derive(Serialize)]
struct SystemInfo {
    os: String,
    arch: String,
    audio_stack: Vec<String>,
    package_manager: Option<String>,
    dependencies: Vec<DependencyInfo>,
}

#[derive(Serialize)]
struct DependencyInfo {
    id: String,
    label: String,
    binary: String,
    installed: bool,
    version: Option<String>,
    purpose: String,
    install_hint: Option<String>,
    update_hint: Option<String>,
    remove_hint: Option<String>,
}

#[derive(Serialize)]
struct DependencyActionResult {
    command: String,
    success: bool,
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

#[derive(Serialize)]
struct RestartResponse {
    message: &'static str,
}

async fn get_system_info() -> Json<SystemInfo> {
    let package_manager = detect_package_manager().await;
    let audio_stack = detect_audio_stack().await;
    let deps = [
        (
            "ffmpeg",
            "FFmpeg",
            "ffmpeg",
            "Decode files, radio streams, devices, TCP wrappers",
        ),
        (
            "shairport-sync",
            "Shairport Sync",
            "shairport-sync",
            "AirPlay receiver source",
        ),
        ("mpd", "MPD", "mpd", "Music Player Daemon FIFO source"),
        (
            "librespot",
            "librespot",
            "librespot",
            "Spotify Connect source",
        ),
    ];

    let mut dependencies = Vec::new();
    for (id, label, binary, purpose) in deps {
        let installed = command_exists(binary).await;
        let version = if installed {
            version_of(binary).await
        } else {
            None
        };
        dependencies.push(DependencyInfo {
            id: id.to_owned(),
            label: label.to_owned(),
            binary: binary.to_owned(),
            installed,
            version,
            purpose: purpose.to_owned(),
            install_hint: package_hint(package_manager.as_deref(), "install", id),
            update_hint: package_hint(package_manager.as_deref(), "update", id),
            remove_hint: package_hint(package_manager.as_deref(), "remove", id),
        });
    }

    Json(SystemInfo {
        os: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        audio_stack,
        package_manager,
        dependencies,
    })
}

async fn get_logs() -> Response {
    let candidates = [
        "/var/log/sonium/server.log",
        "/var/log/sonium/sonium-server.log",
        "./sonium.log",
        "./run/sonium.log",
        "run/sonium.log",
    ];

    for path in candidates {
        if Path::new(path).exists() {
            match tail_file(path, 240).await {
                Ok(logs) => {
                    return ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], logs)
                        .into_response();
                }
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
                }
            }
        }
    }

    if command_exists("journalctl").await {
        for unit in ["sonium-server", "sonium-server.service", "sonium"] {
            match Command::new("journalctl")
                .args(["-u", unit, "-n", "240", "--no-pager"])
                .output()
                .await
            {
                Ok(output) if output.status.success() && !output.stdout.is_empty() => {
                    return (
                        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                        String::from_utf8_lossy(&output.stdout).to_string(),
                    )
                        .into_response();
                }
                _ => {}
            }
        }
        match Command::new("journalctl")
            .args(["-t", "sonium-server", "-n", "240", "--no-pager"])
            .output()
            .await
        {
            Ok(output) if output.status.success() && !output.stdout.is_empty() => {
                return (
                    [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                    String::from_utf8_lossy(&output.stdout).to_string(),
                )
                    .into_response();
            }
            _ => {}
        }
    }

    (
        StatusCode::OK,
        "No readable Sonium log file found. If running under systemd, grant journal access or configure file logging.",
    )
        .into_response()
}

async fn post_restart() -> Json<RestartResponse> {
    tokio::spawn(async {
        tokio::time::sleep(Duration::from_millis(350)).await;
        tracing::warn!("Sonium server restart requested via control API");
        std::process::exit(0);
    });
    Json(RestartResponse {
        message: "Sonium server restart requested",
    })
}

async fn post_dependency_action(AxumPath((id, action)): AxumPath<(String, String)>) -> Response {
    let Some(package_manager) = detect_package_manager().await else {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "No supported package manager detected",
        )
            .into_response();
    };

    let Some(spec) = package_command(&package_manager, &action, &id) else {
        return (
            StatusCode::BAD_REQUEST,
            "Unsupported dependency/action/package-manager combination",
        )
            .into_response();
    };

    let command_label = std::iter::once(spec.program.as_str())
        .chain(spec.args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ");

    match timeout(
        Duration::from_secs(180),
        Command::new(&spec.program).args(&spec.args).output(),
    )
    .await
    {
        Ok(Ok(output)) => Json(DependencyActionResult {
            command: command_label,
            success: output.status.success(),
            status: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
        .into_response(),
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("{command_label}: {e}"),
        )
            .into_response(),
        Err(_) => (
            StatusCode::REQUEST_TIMEOUT,
            format!("{command_label}: command timed out"),
        )
            .into_response(),
    }
}

async fn command_exists(binary: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {binary} >/dev/null 2>&1")])
        .status()
        .await
        .map(|status| status.success())
        .unwrap_or(false)
}

async fn version_of(binary: &str) -> Option<String> {
    let output = Command::new(binary).arg("--version").output().await.ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(|line| line.trim().to_owned())
}

async fn detect_package_manager() -> Option<String> {
    for candidate in ["brew", "apt", "dnf", "pacman", "zypper"] {
        if command_exists(candidate).await {
            return Some(candidate.to_owned());
        }
    }
    None
}

async fn detect_audio_stack() -> Vec<String> {
    let mut stacks = Vec::new();
    if command_exists("pactl").await {
        stacks.push("PulseAudio / PipeWire Pulse".to_owned());
    }
    if command_exists("pw-cli").await || command_exists("wpctl").await {
        stacks.push("PipeWire".to_owned());
    }
    if command_exists("arecord").await || command_exists("aplay").await {
        stacks.push("ALSA".to_owned());
    }
    if std::env::consts::OS == "macos" {
        stacks.push("CoreAudio".to_owned());
    }
    stacks
}

fn package_hint(manager: Option<&str>, action: &str, package: &str) -> Option<String> {
    let pkg = match (manager, package) {
        (Some("brew"), "mpd") => "mpd",
        (Some("apt"), "shairport-sync") => "shairport-sync",
        (Some("apt"), "librespot") => "librespot",
        (_, pkg) => pkg,
    };

    let command = match (manager?, action) {
        ("brew", "install") => format!("brew install {pkg}"),
        ("brew", "update") => format!("brew upgrade {pkg}"),
        ("brew", "remove") => format!("brew uninstall {pkg}"),
        ("apt", "install") => format!("sudo apt install {pkg}"),
        ("apt", "update") => format!("sudo apt install --only-upgrade {pkg}"),
        ("apt", "remove") => format!("sudo apt remove {pkg}"),
        ("dnf", "install") => format!("sudo dnf install {pkg}"),
        ("dnf", "update") => format!("sudo dnf update {pkg}"),
        ("dnf", "remove") => format!("sudo dnf remove {pkg}"),
        ("pacman", "install") => format!("sudo pacman -S {pkg}"),
        ("pacman", "update") => format!("sudo pacman -Syu {pkg}"),
        ("pacman", "remove") => format!("sudo pacman -R {pkg}"),
        ("zypper", "install") => format!("sudo zypper install {pkg}"),
        ("zypper", "update") => format!("sudo zypper update {pkg}"),
        ("zypper", "remove") => format!("sudo zypper remove {pkg}"),
        _ => return None,
    };

    Some(command)
}

struct CommandSpec {
    program: String,
    args: Vec<String>,
}

fn package_command(manager: &str, action: &str, package: &str) -> Option<CommandSpec> {
    let pkg = package_name(manager, package)?;
    let (program, args): (&str, Vec<&str>) = match (manager, action) {
        ("brew", "install") => ("brew", vec!["install", pkg]),
        ("brew", "update") => ("brew", vec!["upgrade", pkg]),
        ("brew", "remove") => ("brew", vec!["uninstall", pkg]),
        ("apt", "install") => ("sudo", vec!["-n", "apt-get", "install", "-y", pkg]),
        ("apt", "update") => (
            "sudo",
            vec!["-n", "apt-get", "install", "--only-upgrade", "-y", pkg],
        ),
        ("apt", "remove") => ("sudo", vec!["-n", "apt-get", "remove", "-y", pkg]),
        ("dnf", "install") => ("sudo", vec!["-n", "dnf", "install", "-y", pkg]),
        ("dnf", "update") => ("sudo", vec!["-n", "dnf", "upgrade", "-y", pkg]),
        ("dnf", "remove") => ("sudo", vec!["-n", "dnf", "remove", "-y", pkg]),
        ("pacman", "install") => ("sudo", vec!["-n", "pacman", "-S", "--noconfirm", pkg]),
        ("pacman", "update") => ("sudo", vec!["-n", "pacman", "-Syu", "--noconfirm", pkg]),
        ("pacman", "remove") => ("sudo", vec!["-n", "pacman", "-R", "--noconfirm", pkg]),
        ("zypper", "install") => (
            "sudo",
            vec!["-n", "zypper", "--non-interactive", "install", pkg],
        ),
        ("zypper", "update") => (
            "sudo",
            vec!["-n", "zypper", "--non-interactive", "update", pkg],
        ),
        ("zypper", "remove") => (
            "sudo",
            vec!["-n", "zypper", "--non-interactive", "remove", pkg],
        ),
        _ => return None,
    };

    Some(CommandSpec {
        program: program.to_owned(),
        args: args.into_iter().map(str::to_owned).collect(),
    })
}

fn package_name(manager: &str, package: &str) -> Option<&'static str> {
    match (manager, package) {
        (_, "ffmpeg") => Some("ffmpeg"),
        (_, "shairport-sync") => Some("shairport-sync"),
        (_, "mpd") => Some("mpd"),
        (_, "librespot") => Some("librespot"),
        _ => None,
    }
}

async fn tail_file(path: &str, lines: usize) -> std::io::Result<String> {
    let content = tokio::fs::read_to_string(path).await?;
    let mut tail = content.lines().rev().take(lines).collect::<Vec<_>>();
    tail.reverse();
    Ok(tail.join("\n"))
}
