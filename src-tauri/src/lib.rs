//! YOLA Desktop — Tauri v2 wrapper: si-yola + yola-daemon sidecar.
//! Boot: splash → launch daemon → health check → show window.
//! Watchdog: daemon muerto = desktop muerto.
//! Cleanup: cierra ventana = mata daemon.

use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::Manager;

struct DaemonState {
    child: Mutex<Option<Child>>,
}

const DAEMON_PORT: u16 = 7779;
const DAEMON_HEALTH_TIMEOUT_SECS: u64 = 30;
const DAEMON_POLL_MS: u64 = 500;

// ── Health Check ─────────────────────────────────────────────────────────

fn check_daemon_health(port: u16) -> bool {
    let url = format!("http://localhost:{}/api/v1/health", port);
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    client.get(&url).send().ok().map(|r| r.status().is_success()).unwrap_or(false)
}

// ── Find Sidecar ─────────────────────────────────────────────────────────

fn find_sidecar() -> Option<PathBuf> {
    let exe_path = std::env::current_exe().ok()?;
    let exe_dir = exe_path.parent()?;
    let daemon_name = format!("yola-daemon{}", std::env::consts::EXE_SUFFIX);

    for path in &[
        exe_dir.join(&daemon_name),
        exe_dir.join("resources").join(&daemon_name),
        exe_dir.join("target/release").join(&daemon_name),
    ] {
        if path.exists() {
            return Some(path.clone());
        }
    }
    None
}

// ── Launch Daemon ────────────────────────────────────────────────────────

fn launch_daemon(path: &std::path::Path, port: u16) -> Result<Child, String> {
    let mut cmd = Command::new(path);
    cmd.args(["start", "--port", &port.to_string(), "--foreground"]);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }

    cmd.spawn().map_err(|e| format!("No se pudo iniciar el daemon: {}", e))
}

// ── Wait for Daemon ──────────────────────────────────────────────────────

fn wait_for_daemon(port: u16) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(DAEMON_HEALTH_TIMEOUT_SECS) {
        if check_daemon_health(port) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(DAEMON_POLL_MS));
    }
    false
}

// ── Kill Daemon ──────────────────────────────────────────────────────────

fn kill_daemon(state: &DaemonState) {
    if let Ok(mut guard) = state.child.lock() {
        if let Some(ref mut child) = *guard {
            eprintln!("[YOLA] Matando daemon (PID {})...", child.id());
            let _ = child.kill();
            let _ = child.wait();
            *guard = None;
        }
    }
}

// ── Tauri Entry Point ────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // 1. ¿Daemon ya corriendo?
            if !check_daemon_health(DAEMON_PORT) {
                let path = find_sidecar().unwrap_or_else(|| {
                    eprintln!("[YOLA] No se encontró yola-daemon junto al ejecutable.");
                    std::process::exit(1);
                });
                let child = launch_daemon(&path, DAEMON_PORT).unwrap_or_else(|e| {
                    eprintln!("[YOLA] Error lanzando daemon: {}", e);
                    std::process::exit(1);
                });

                // Guardar child para matarlo al cerrar
                if let Ok(mut guard) = app.state::<DaemonState>().child.lock() {
                    *guard = Some(child);
                }

                // Esperar health
                if !wait_for_daemon(DAEMON_PORT) {
                    eprintln!("[YOLA] Timeout esperando daemon.");
                    kill_daemon(&app.state::<DaemonState>());
                    std::process::exit(1);
                }
            }

            // 2. Watchdog: daemon muerto = cerrar app
            let handle = app.handle().clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(15)).await; // grace period
                let mut failures = 0u32;
                loop {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    if check_daemon_health(DAEMON_PORT) {
                        failures = 0;
                    } else {
                        failures += 1;
                    }
                    if failures >= 2 {
                        eprintln!("[YOLA] Daemon perdido. Cerrando.");
                        handle.exit(1);
                    }
                }
            });

            // 3. Mostrar ventana (ya estaba oculta)
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                kill_daemon(&window.state::<DaemonState>());
            }
        })
        .manage(DaemonState { child: Mutex::new(None) })
        .run(tauri::generate_context!())
        .expect("error fatal al ejecutar YOLA Desktop");
}
