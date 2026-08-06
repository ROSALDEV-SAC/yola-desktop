//! YOLA Desktop — Tauri v2 wrapper que orquesta si-yola + yola-daemon.
//!
//! Flujo de arranque:
//! 1. Chequea health del daemon en :7779 (timeout 2s)
//! 2. Si no responde: busca yola-daemon sidecar junto al ejecutable
//! 3. Si no existe sidecar: error nativo y cierre
//! 4. Si existe: lanza proceso con --port 7779 --foreground
//! 5. Espera en loop (máx 15s, 500ms) hasta health OK
//! 6. Muestra ventana principal con si-yola
//! 7. Al cerrar ventana: mata el proceso daemon hijo

use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::Manager;

/// Estado compartido: guarda el proceso hijo del daemon (si fue lanzado por nosotros).
struct DaemonState {
    child: Mutex<Option<Child>>,
}

const DAEMON_PORT: u16 = 7779;
const DAEMON_TIMEOUT_SECS: u64 = 15;
const DAEMON_POLL_MS: u64 = 500;

// ── Health Check ────────────────────────────────────────────────────────────

/// Verifica si el daemon responde en el puerto dado.
/// Usa reqwest::blocking (no async) — compatible con el hook setup de Tauri v2.
fn check_daemon_health(port: u16) -> bool {
    let url = format!("http://localhost:{}/api/v1/health", port);
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };

    match client.get(&url).send() {
        Ok(resp) => {
            if resp.status().is_success() {
                resp.json::<serde_json::Value>()
                    .ok()
                    .and_then(|v| v.get("healthy")?.as_bool())
                    .unwrap_or(false)
            } else {
                false
            }
        }
        Err(_) => false,
    }
}

// ── Sidecar Discovery ───────────────────────────────────────────────────────

/// Busca el binario yola-daemon en el mismo directorio que el ejecutable
/// principal. Cross-platform: usa EXE_SUFFIX (.exe en Windows, vacío en
/// Linux/Mac).
fn find_sidecar() -> Option<PathBuf> {
    let exe_path = std::env::current_exe().ok()?;
    let exe_dir = exe_path.parent()?;

    let daemon_name = format!("yola-daemon{}", std::env::consts::EXE_SUFFIX);

    // Buscar en múltiples ubicaciones (orden de prioridad):
    let candidates = vec![
        exe_dir.join(&daemon_name),                           // mismo dir que el .exe
        exe_dir.join("resources").join(&daemon_name),          // Tauri resources/
        exe_dir.join("target").join("release").join(&daemon_name), // legacy resources
        exe_dir.parent().unwrap_or(exe_dir).join(&daemon_name),
    ];

    for path in candidates {
        if path.exists() {
            return Some(path);
        }
    }
    None
}

// ── Daemon Launch ───────────────────────────────────────────────────────────

/// Lanza el daemon como proceso hijo con --port y --foreground.
fn launch_daemon(sidecar_path: &std::path::Path, port: u16) -> Result<Child, String> {
    Command::new(sidecar_path)
        .args(["start", "--port", &port.to_string(), "--foreground"])
        .spawn()
        .map_err(|e| format!("No se pudo iniciar el daemon: {}", e))
}

/// Espera en loop hasta que el daemon responda al health check.
fn wait_for_daemon(port: u16, max_wait: Duration, poll_interval: Duration) -> bool {
    let start = Instant::now();
    loop {
        if check_daemon_health(port) {
            return true;
        }
        if start.elapsed() >= max_wait {
            return false;
        }
        std::thread::sleep(poll_interval);
    }
}

// ── Daemon Lifecycle ────────────────────────────────────────────────────────

/// Mata el proceso hijo del daemon si existe.
fn kill_daemon(state: &DaemonState) {
    if let Ok(mut guard) = state.child.lock() {
        if let Some(ref mut child) = *guard {
            eprintln!("[YOLA] Deteniendo daemon (PID {})...", child.id());
            let _ = child.kill();
            let _ = child.wait();
            *guard = None;
            eprintln!("[YOLA] Daemon detenido.");
        }
    }
}

// ── Tauri App Entry Point ───────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            println!("[YOLA] Iniciando YOLA Desktop...");

            // 1. Chequear si el daemon ya está corriendo
            println!("[YOLA] Verificando conexión al daemon en puerto {}...", DAEMON_PORT);

            if !check_daemon_health(DAEMON_PORT) {
                println!("[YOLA] Daemon no responde. Buscando sidecar...");

                // 2. Buscar sidecar
                let sidecar_path = match find_sidecar() {
                    Some(p) => p,
                    None => {
                        let msg = format!(
                            "No se encontró yola-daemon{} junto al ejecutable.\n\
                             Coloca el binario en el mismo directorio y reintenta.",
                            std::env::consts::EXE_SUFFIX
                        );
                        eprintln!("[YOLA] ERROR: {}", msg);
                        // TODO: reemplazar con diálogo nativo (rfd::MessageDialog)
                        std::process::exit(1);
                    }
                };

                println!("[YOLA] Sidecar encontrado: {}", sidecar_path.display());
                println!("[YOLA] Iniciando daemon...");

                // 3. Lanzar daemon como proceso hijo
                let child = match launch_daemon(&sidecar_path, DAEMON_PORT) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("[YOLA] ERROR: {}", e);
                        std::process::exit(1);
                    }
                };

                println!("[YOLA] Daemon lanzado (PID {}).", child.id());

                // Guardar el proceso hijo en estado gestionado
                let state = app.state::<DaemonState>();
                *state.child.lock().unwrap() = Some(child);

                // 4. Esperar a que el daemon esté listo
                println!("[YOLA] Conectando al daemon...");
                let ready = wait_for_daemon(
                    DAEMON_PORT,
                    Duration::from_secs(DAEMON_TIMEOUT_SECS),
                    Duration::from_millis(DAEMON_POLL_MS),
                );

                if !ready {
                    eprintln!(
                        "[YOLA] ERROR: El daemon no respondió en {} segundos.",
                        DAEMON_TIMEOUT_SECS
                    );
                    let state = app.state::<DaemonState>();
                    kill_daemon(&state);
                    std::process::exit(1);
                }

                println!("[YOLA] Daemon listo.");
            } else {
                println!("[YOLA] Daemon ya está corriendo.");
            }

            println!("[YOLA] Listo. Abriendo YOLA...");

            // Mostrar la ventana principal (estaba oculta: visible=false en config)
            if let Some(window) = app.get_webview_window("main") {
                window.show().map_err(|e| format!("No se pudo mostrar la ventana: {}", e))?;
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                eprintln!("[YOLA] Ventana cerrada. Limpiando...");
                let state = window.state::<DaemonState>();
                kill_daemon(&state);
            }
        })
        .manage(DaemonState {
            child: Mutex::new(None),
        })
        .run(tauri::generate_context!())
        .expect("error fatal al ejecutar YOLA Desktop");
}
