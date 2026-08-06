# CODE_MAP.md — yola-desktop

> Documentación generada desde el código real en disco (filesystem = verdad).
> Proyecto: `D:\Workspace Miaw\CUERPOS_YOLA\yola-desktop\`

---

## 1. STACK

| Capa              | Tecnología                       | Versión     |
|-------------------|----------------------------------|-------------|
| Shell nativa      | **Tauri v2** (Rust)              | 2.x         |
| Lenguaje backend  | **Rust** (edition 2021)          | stable      |
| Frontend          | **si-yola** (SolidJS + Vite)     | —           |
| Package manager   | **Bun**                          | latest      |
| HTTP client       | **reqwest** 0.12 (blocking+json) | 0.12        |
| Serialización     | **serde** 1 (derive) + serde_json| 1.x         |
| Async runtime     | **tokio** 1 (full)               | 1.x         |
| CLI               | `@tauri-apps/cli` ^2             | 2.x         |
| Icon utils (JS)   | `png-to-icns`, `sharp`, `to-ico` | —            |
| CI                | GitHub Actions                   | ubuntu/macos/windows-latest |

---

## 2. ENTRY POINT

### `src-tauri/src/main.rs` (6 líneas)

```rust
// L1: Previene ventana de consola en Windows release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// L4-6: Único propósito: delegar en lib::run()
fn main() {
    yola_desktop_lib::run();
}
```

**Explicación línea a línea:**

| Línea | Qué hace |
|-------|----------|
| `L1-2` | Atributo condicional: en Windows con `--release`, oculta la ventana de consola. En debug o Linux/Mac no tiene efecto. |
| `L4-6` | `main()` llama a `yola_desktop_lib::run()`. Toda la lógica real está en `lib.rs`. `main.rs` es un bootstrap puro. |

**Por qué existe `main.rs` separado de `lib.rs`:**
- `Cargo.toml` declara `crate-type = ["staticlib", "cdylib", "rlib"]`. El crate es una librería.
- Tauri necesita un binario con `fn main()`. Separar `main.rs` (bin) de `lib.rs` (lib) permite que el código sea testeable como librería y que Tauri lo referencie como `yola_desktop_lib::run()`.

---

## 3. MÓDULOS — Exhaustivo

### 3.1 `src-tauri/Cargo.toml` — Dependencias Rust (19 líneas)

```toml
[package]
name = "yola-desktop"
version = "0.1.0"
description = "YOLA Desktop — si-yola + yola-daemon empaquetados con Tauri v2"
edition = "2021"

[lib]
name = "yola_desktop_lib"           # ← nombre del crate, usado en main.rs
crate-type = ["staticlib", "cdylib", "rlib"]  # ← 3 tipos: lib estática, C dinámica, Rust lib

[build-dependencies]
tauri-build = { version = "2", features = [] }   # ← usado en build.rs para generar contexto

[dependencies]
tauri = { version = "2", features = [] }          # ← framework Tauri v2
serde = { version = "1", features = ["derive"] }  # ← serialización
serde_json = "1"                                   # ← parseo JSON del health check
reqwest = { version = "0.12", features = ["blocking", "json"] }  # ← HTTP cliente síncrono
tokio = { version = "1", features = ["full"] }     # ← runtime async (requerido por Tauri)
```

**Detalle de cada dependencia y por qué está:**

| Dependencia | Features | Rol en el proyecto |
|-------------|----------|---------------------|
| `tauri` v2 | — | Framework completo: ventana nativa, ciclo de vida, state management, eventos |
| `serde` v1 | `derive` | Macro `#[derive(Serialize, Deserialize)]` — aunque no se usa explícitamente en lib.rs, es transitivo de Tauri |
| `serde_json` v1 | — | `resp.json::<serde_json::Value>()` en `check_daemon_health()` — parsea la respuesta JSON del daemon |
| `reqwest` v0.12 | `blocking`, `json` | `reqwest::blocking::Client` — cliente HTTP síncrono. `blocking` porque `setup()` de Tauri no es async. `json` para `resp.json()` |
| `tokio` v1 | `full` | Runtime async requerido por Tauri internamente (webview, eventos, IPC) |
| `tauri-build` v2 | — | Build script dependency. `tauri_build::build()` en `build.rs` genera el contexto Tauri (capacidades, esquemas, permisos) |

---

### 3.2 `src-tauri/build.rs` (3 líneas)

```rust
fn main() {
    tauri_build::build()   // ← genera código en tiempo de compilación:
}                          //    tauri::generate_context!() lo consume en lib.rs L212
```

**Qué genera `tauri_build::build()`:**
- Lee `tauri.conf.json`
- Lee los archivos en `src-tauri/gen/schemas/` (acl-manifests.json, capabilities.json, desktop-schema.json, windows-schema.json)
- Genera código Rust que incluye estos recursos embebidos en el binario
- `tauri::generate_context!()` en lib.rs L212 expande a este código generado

---

### 3.3 `src-tauri/src/lib.rs` — Núcleo de la aplicación (214 líneas)

> **Archivo más importante del proyecto.** Toda la lógica de arranque, sidecar discovery, lanzamiento del daemon, health check, ciclo de vida de ventana y cleanup está aquí.

#### 3.3.1 Header doc (L1-10)

```rust
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
```

#### 3.3.2 Imports (L12-17)

```rust
use std::path::PathBuf;           // ← ruta del sidecar
use std::process::{Child, Command}; // ← lanzar y matar proceso daemon
use std::sync::Mutex;             // ← acceso thread-safe al Child handle
use std::time::{Duration, Instant}; // ← timeouts y polling
use tauri::Manager;               // ← trait que habilita app.state() y app.get_webview_window()
```

#### 3.3.3 DaemonState (L20-22)

```rust
struct DaemonState {
    child: Mutex<Option<Child>>,  // ← Option porque puede ser None si el daemon ya corría
}                                 //    Mutex porque Tauri es multi-thread (event loop + webview)
```

**Propósito:** Almacenar el handle `Child` del proceso daemon lanzado. Necesario para:
- Matarlo en `on_window_event::CloseRequested` (L203-207)
- Matarlo si falla el health check durante el arranque (L183-185)
- `Mutex<Option<Child>>`: `Mutex` porque Tauri puede acceder desde distintos threads. `Option` porque si el daemon ya estaba corriendo al abrir la app, no lanzamos proceso hijo y no hay Child que matar.

#### 3.3.4 Constantes (L24-26)

```rust
const DAEMON_PORT: u16 = 7779;          // ← puerto donde el daemon expone /api/v1/health
const DAEMON_TIMEOUT_SECS: u64 = 15;    // ← tiempo máximo de espera en wait_for_daemon()
const DAEMON_POLL_MS: u64 = 500;        // ← intervalo entre health checks en el loop
```

#### 3.3.5 `check_daemon_health()` (L32-55) — LÍNEA A LÍNEA

```rust
fn check_daemon_health(port: u16) -> bool {
```
**L32:** Función pura, síncrona. Recibe puerto, retorna `bool`. Es `blocking` porque `setup()` de Tauri no soporta async.

```rust
    let url = format!("http://localhost:{}/api/v1/health", port);
```
**L33:** Construye la URL del health endpoint. El daemon debe exponer `GET /api/v1/health` que retorna `{"healthy": true/false}`.

```rust
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
```
**L34-40:** Crea cliente HTTP síncrono con timeout de 2s. Si falla la construcción del cliente (ej: problemas de TLS), retorna `false`. El timeout de 2s es agresivo para no bloquear el arranque.

```rust
    match client.get(&url).send() {
```
**L42:** Envía GET síncrono.

```rust
        Ok(resp) => {
            if resp.status().is_success() {           // 2xx
                resp.json::<serde_json::Value>()      // parsea JSON
                    .ok()                              // Result → Option
                    .and_then(|v| v.get("healthy")?.as_bool())  // extrae campo "healthy"
                    .unwrap_or(false)                  // si algo falla → false
            } else {
                false                                 // status != 2xx
            }
        }
        Err(_) => false,                              // error de red/timeout
    }
```
**L43-54:** Validación en cascada:
1. Status code 2xx? → no → `false`
2. Parseo JSON exitoso? → no → `false`
3. Campo `"healthy"` existe y es `bool`? → no → `false`
4. `"healthy"` es `true`? → sí → `true`

**Por qué `serde_json::Value` y no una struct tipada:** Evita una dependencia de schema. El daemon puede evolucionar su respuesta sin romper el desktop mientras el campo `healthy` siga existiendo.

#### 3.3.6 `find_sidecar()` (L62-82) — LÍNEA A LÍNEA

```rust
fn find_sidecar() -> Option<PathBuf> {
```
**L62:** Retorna `Option<PathBuf>` — `None` si no se encuentra el binario en ninguna ubicación.

```rust
    let exe_path = std::env::current_exe().ok()?;     // ← falla temprano si no puede leer ruta
    let exe_dir = exe_path.parent()?;                  // ← directorio del .exe/.app
```
**L63-64:** `current_exe()` obtiene la ruta absoluta del ejecutable YOLA Desktop. `parent()` obtiene el directorio contenedor. Ambos usan `?` para retornar `None` si fallan.

```rust
    let daemon_name = format!("yola-daemon{}", std::env::consts::EXE_SUFFIX);
```
**L66:** `EXE_SUFFIX` es `".exe"` en Windows, `""` en Linux/Mac. Resultado: `"yola-daemon.exe"` o `"yola-daemon"`.

```rust
    let candidates = vec![
        exe_dir.join(&daemon_name),                                        // ① mismo dir
        exe_dir.join("resources").join(&daemon_name),                      // ② Tauri resources/
        exe_dir.join("target").join("release").join(&daemon_name),         // ③ legacy (dev)
        exe_dir.parent().unwrap_or(exe_dir).join(&daemon_name),            // ④ un nivel arriba
    ];
```
**L69-74:** **Cuatro ubicaciones en orden de prioridad:**

| # | Ruta | Contexto |
|---|------|----------|
| ① | `exe_dir/yola-daemon(.exe)` | Mismo directorio que el ejecutable. Caso más común en desarrollo. |
| ② | `exe_dir/resources/yola-daemon(.exe)` | Carpeta `resources/` de Tauri bundle. El `tauri.conf.json` L31-33 declara `"resources": ["yola-daemon.exe"]`, Tauri lo copia a `resources/` en el bundle. |
| ③ | `exe_dir/target/release/yola-daemon(.exe)` | Legacy: ubicación típica de `cargo build --release`. |
| ④ | `exe_dir/../yola-daemon(.exe)` | Un nivel arriba del directorio del exe. Caso: estructura de workspace. |

```rust
    for path in candidates {
        if path.exists() {          // ← verificación en disco (filesystem = verdad)
            return Some(path);
        }
    }
    None
```
**L76-81:** Itera en orden. Primero que existe → retorna. Si ninguno existe → `None`.

#### 3.3.7 `launch_daemon()` (L87-92) — LÍNEA A LÍNEA

```rust
fn launch_daemon(sidecar_path: &std::path::Path, port: u16) -> Result<Child, String> {
```
**L87:** Recibe ruta verificada del binario y puerto. Retorna `Result<Child, String>` — el handle del proceso o mensaje de error.

```rust
    Command::new(sidecar_path)
        .args(["start", "--port", &port.to_string(), "--foreground"])
        .spawn()
        .map_err(|e| format!("No se pudo iniciar el daemon: {}", e))
```
**L88-92:** Ejecuta: `yola-daemon start --port 7779 --foreground`

| Argumento | Significado |
|-----------|-------------|
| `start` | Subcomando del daemon CLI |
| `--port 7779` | Puerto donde escuchará |
| `--foreground` | Corre en primer plano (no demoniza). Necesario para que `Child` pueda rastrearlo y matarlo. |

**Por qué `--foreground` es crítico:** Si el daemon se demoniza (fork + setsid), el proceso padre muere inmediatamente, `Child` queda huérfano y `kill_daemon()` no puede detenerlo. `--foreground` mantiene al daemon como hijo directo.

#### 3.3.8 `wait_for_daemon()` (L95-106) — LÍNEA A LÍNEA

```rust
fn wait_for_daemon(port: u16, max_wait: Duration, poll_interval: Duration) -> bool {
```
**L95:** Recibe puerto, tiempo máximo y intervalo de polling. Retorna `true` si el daemon respondió a tiempo.

```rust
    let start = Instant::now();        // ← marca el inicio
    loop {
        if check_daemon_health(port) { // ← health check (2s timeout cada intento)
            return true;               // ← éxito
        }
        if start.elapsed() >= max_wait { // ← ¿se agotó el tiempo total?
            return false;              // ← timeout
        }
        std::thread::sleep(poll_interval); // ← espera 500ms antes de reintentar
    }
```
**L96-106:** Loop bloqueante con early exit:
- Cada iteración: `check_daemon_health()` (bloquea hasta 2s)
- Verifica tiempo transcurrido contra `max_wait` (15s)
- Duerme 500ms entre intentos
- **Peor caso:** ~7-8 intentos × 2s timeout + 500ms sleep ≈ 17-20s reales

**Nota sobre `std::thread::sleep`:** Es bloqueante para el thread actual. Como `setup()` de Tauri corre en un thread dedicado antes de iniciar el event loop, esto es aceptable. No bloquea la UI porque la ventana aún no se muestra.

#### 3.3.9 `kill_daemon()` (L111-121) — LÍNEA A LÍNEA

```rust
fn kill_daemon(state: &DaemonState) {
```
**L111:** Recibe referencia al estado compartido.

```rust
    if let Ok(mut guard) = state.child.lock() {       // ← adquiere el Mutex
        if let Some(ref mut child) = *guard {          // ← ¿hay Child?
            eprintln!("[YOLA] Deteniendo daemon (PID {})...", child.id());
            let _ = child.kill();                      // ← SIGKILL / TerminateProcess
            let _ = child.wait();                      // ← espera a que termine (reap)
            *guard = None;                             // ← limpia el estado
            eprintln!("[YOLA] Daemon detenido.");
        }
    }
```
**L112-121:** Secuencia de apagado:
1. `lock()` → adquiere Mutex. Si está envenenado (panic previo), `if let Ok` falla silenciosamente.
2. `if let Some(ref mut child)` → solo actúa si hay un proceso hijo (si el daemon ya corría, es `None`).
3. `child.kill()` → envía señal de terminación. En Windows: `TerminateProcess`. En Unix: `SIGKILL`.
4. `child.wait()` → recolecta el proceso zombie (reap). Sin esto, queda como proceso zombie hasta que el padre termine.
5. `*guard = None` → limpia el estado para evitar double-kill.

**Se llama desde dos lugares:**
- `setup()` L183-184: si el daemon no responde en 15s, mata el hijo y sale.
- `on_window_event` L206: al cerrar la ventana principal.

#### 3.3.10 `run()` — Tauri App Entry Point (L126-213) — LÍNEA A LÍNEA

```rust
#[cfg_attr(mobile, tauri::mobile_entry_point)]  // ← si compila para mobile, usa mobile_entry_point
pub fn run() {
```
**L126-127:** Atributo condicional para mobile (no usado en desktop, pero requerido por Tauri).

```rust
    tauri::Builder::default()
```
**L128:** Inicia el builder de Tauri. `default()` configura valores por defecto para window management, IPC, security.

```rust
        .setup(|app| {
```
**L129:** Closure `setup` — se ejecuta una vez al inicio, antes de mostrar ventanas. Es síncrono. `app` es `&mut App`.

```rust
            println!("[YOLA] Iniciando YOLA Desktop...");
```
**L130:** Log de arranque.

```rust
            // 1. Chequear si el daemon ya está corriendo
            println!("[YOLA] Verificando conexión al daemon en puerto {}...", DAEMON_PORT);
            if !check_daemon_health(DAEMON_PORT) {
```
**L132-134:** **Paso 1.** Health check. Si el daemon YA responde (ej: estaba corriendo de una sesión anterior), se salta todo el bloque de lanzamiento y va directo a mostrar la ventana (L193).

```rust
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
```
**L135-150:** **Paso 2.** `find_sidecar()` busca en 4 ubicaciones. Si retorna `None`: imprime error en stderr, sale con código 1. El `TODO` indica intención futura de usar `rfd` (Rust File Dialog) para mostrar un diálogo nativo en vez de stderr.

```rust
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
```
**L152-165:** **Paso 3.** Lanza el proceso. Si falla (binario corrupto, permisos, etc.), sale con error. Si éxito, loguea el PID.

```rust
                // Guardar el proceso hijo en estado gestionado
                let state = app.state::<DaemonState>();
                *state.child.lock().unwrap() = Some(child);
```
**L167-168:** Guarda el `Child` en `DaemonState` para que `kill_daemon()` pueda accederlo después. `app.state::<DaemonState>()` obtiene referencia al estado gestionado por Tauri (registrado en L209-211 con `.manage()`). `lock().unwrap()` adquiere el Mutex — `unwrap()` es aceptable aquí porque un panic en setup es fatal de todos modos.

```rust
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
```
**L170-189:** **Paso 4.** Polling loop (máx 15s, cada 500ms). Si timeout: mata el daemon (cleanup), loguea error, sale con código 1.

```rust
            } else {
                println!("[YOLA] Daemon ya está corriendo.");
            }
```
**L189-191:** Rama alternativa del `if !check_daemon_health()` en L134: el daemon ya respondía, no hizo falta lanzarlo. `DaemonState.child` queda como `None`, lo que significa que `kill_daemon()` al cerrar no intentará matar nada.

```rust
            println!("[YOLA] Listo. Abriendo YOLA...");

            // Mostrar la ventana principal (estaba oculta: visible=false en config)
            if let Some(window) = app.get_webview_window("main") {
                window.show().map_err(|e| format!("No se pudo mostrar la ventana: {}", e))?;
            }
```
**L193-198:** **Paso 5.** `app.get_webview_window("main")` busca la ventana con label `"main"` (definida en tauri.conf.json L20). La ventana se creó oculta (`"visible": false` en config L21) — aquí se muestra. `?` propaga el error al caller (Tauri), que mostrará un diálogo de error nativo.

```rust
            Ok(())
        })
```
**L200-201:** `setup` retorna `Ok(())` — Tauri procede a iniciar el event loop.

```rust
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                eprintln!("[YOLA] Ventana cerrada. Limpiando...");
                let state = window.state::<DaemonState>();
                kill_daemon(&state);
            }
        })
```
**L202-208:** **Hook de cierre de ventana.** Escucha eventos de todas las ventanas. Al detectar `CloseRequested`:
1. Obtiene `DaemonState` desde el scope de la ventana (compartido con la app)
2. Llama a `kill_daemon()` — si `child` es `Some`, mata el proceso; si es `None`, no-op.

**Por qué `on_window_event` y no `on_exit`:** `CloseRequested` se dispara antes de que la ventana se cierre. Si se usara `on_exit`, el daemon quedaría corriendo si la app crashea o se cierra forzosamente. Este hook garantiza cleanup incluso si el cierre es iniciado por el usuario.

```rust
        .manage(DaemonState {
            child: Mutex::new(None),
        })
```
**L209-211:** Registra `DaemonState` en el state manager de Tauri. A partir de aquí, `app.state::<DaemonState>()` y `window.state::<DaemonState>()` funcionan.

```rust
        .run(tauri::generate_context!())
        .expect("error fatal al ejecutar YOLA Desktop");
```
**L212-213:** `tauri::generate_context!()` expande al código generado por `tauri_build::build()` en `build.rs`. Incluye la configuración de `tauri.conf.json`, capacidades, esquemas de permisos, iconos, etc. `run()` inicia el event loop. `expect()` hace panic si hay error fatal (ej: no se pudo crear la ventana nativa).

---

### 3.4 `src-tauri/tauri.conf.json` — Configuración Tauri (42 líneas)

```jsonc
{
  "$schema": "https://...",                        // ← schema para autocompletado en IDEs

  // ── Identidad ──
  "productName": "YOLA",                          // ← nombre visible en menú, barra de título, bundle
  "version": "0.1.0",                             // ← versión semántica
  "identifier": "com.yolabysayri.yola-desktop",   // ← bundle ID (único global, reverse-domain)

  // ── Build ──
  "build": {
    "frontendDist": "../../si-yola/dist",         // ← carpeta con SolidJS compilado (relativa a src-tauri/)
    "devUrl": "http://localhost:5173",            // ← Vite dev server en modo desarrollo
    "beforeDevCommand": "",                       // ← vacío: no se ejecuta comando antes de tauri dev
    "beforeBuildCommand": ""                      // ← vacío: el build de si-yola se hace en CI (build.yml L26-30)
  },

  // ── Ventana ──
  "app": {
    "windows": [{
      "title": "YOLA",                            // ← título de la ventana nativa
      "width": 1200,                              // ← ancho inicial en píxeles
      "height": 800,                              // ← alto inicial en píxeles
      "resizable": true,                          // ← el usuario puede redimensionar
      "decorations": true,                        // ← barra de título nativa (minimizar, maximizar, cerrar)
      "label": "main",                            // ← identificador interno (usado en lib.rs L196)
      "visible": false                            // ← CRÍTICO: ventana oculta al inicio → se muestra en lib.rs L197
    }],
    "security": {
      "csp": null                                 // ← Content Security Policy deshabilitada
    }
  },

  // ── Bundle ──
  "bundle": {
    "active": true,
    "targets": ["msi", "nsis", "dmg", "deb", "appimage"],  // ← 5 formatos de instalador
    "resources": ["yola-daemon.exe"],                       // ← sidecar incluido en el bundle
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",                               // ← retina/HiDPI
      "icons/icon.icns",                                    // ← macOS
      "icons/icon.ico"                                      // ← Windows
    ]
  }
}
```

**Tabla de targets de bundle:**

| Target | SO | Formato | Usado en CI |
|--------|----|---------|-------------|
| `msi` | Windows | Windows Installer (.msi) | Artifact no subido (ver nota) |
| `nsis` | Windows | NSIS installer (.exe) | `build.yml` L49 → `yola-desktop-windows` |
| `dmg` | macOS | Apple Disk Image (.dmg) | `build.yml` L56 → `yola-desktop-macos` |
| `deb` | Linux | Debian package (.deb) | Generado pero no subido como artifact separado |
| `appimage` | Linux | AppImage portable | `build.yml` L63 → `yola-desktop-linux` |

**Nota sobre `visible: false`:** Esencial para el flujo. Si fuera `true`, la ventana se mostraría inmediatamente con si-yola sin daemon → pantalla en blanco/error. El valor `false` permite que `setup()` termine el health check y llame a `window.show()` solo cuando el backend está listo.

**Nota sobre `csp: null`:** Deshabilita Content Security Policy. si-yola puede necesitar conexiones WebSocket al daemon, fuentes externas, o eval() para ciertas features. En producción, idealmente se definiría una CSP restrictiva.

**Nota sobre `beforeDevCommand` vacío:** En desarrollo, el usuario debe iniciar Vite manualmente (`bun run dev` en `../si-yola`) antes de `bun run tauri dev`. El CI buildea si-yola explícitamente (L26-30).

---

### 3.5 `package.json` (19 líneas)

```jsonc
{
  "name": "yola-desktop",
  "private": true,                             // ← no publicable en npm
  "version": "0.1.0",
  "description": "YOLA Desktop — Empaquetador final: si-yola + yola-daemon en app nativa",
  "type": "module",                            // ← ES modules

  "scripts": {
    "dev": "tauri dev",                        // ← bun run dev → tauri dev (hot reload + Vite)
    "build": "tauri build"                     // ← bun run build → compila Rust + empaqueta
  },

  "devDependencies": {
    "@tauri-apps/cli": "^2"                   // ← CLI de Tauri (tauri dev, tauri build)
  },

  "dependencies": {
    "png-to-icns": "^1.0.0",                  // ← convierte PNG a .icns (icono macOS)
    "sharp": "^0.35.3",                        // ← manipulación de imágenes (redimensionar iconos)
    "to-ico": "^1.1.5"                        // ← convierte PNG a .ico (icono Windows)
  }
}
```

**Nota sobre las dependencias de iconos:** `png-to-icns`, `sharp`, y `to-ico` son para scripts de build de iconos (probablemente en `scripts/generate-icons.js` o similar). No son dependencias de runtime — la app empaquetada no carga Node.js. Los iconos finales están en `src-tauri/icons/`.

---

### 3.6 `.gitignore` (2 líneas)

```
node_modules/
src-tauri/target/
```

---

## 4. FLUJO DE ARRANQUE — Paso a paso

```
┌─────────────────────────────────────────────────┐
│           main() → yola_desktop_lib::run()       │
└──────────────────────┬──────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────┐
│ SETUP HOOK (síncrono, antes de mostrar ventana) │
├─────────────────────────────────────────────────┤
│                                                 │
│  ① check_daemon_health(7779)                    │
│     │                                           │
│     ├── true ──────────────────────┐            │
│     │   "Daemon ya está corriendo" │            │
│     │                              │            │
│     └── false                      │            │
│          │                         │            │
│          ▼                         │            │
│  ② find_sidecar()                  │            │
│     Busca en 4 ubicaciones:        │            │
│     ├── mismo dir                  │            │
│     ├── resources/                 │            │
│     ├── target/release/ (legacy)   │            │
│     └── ../                        │            │
│     │                              │            │
│     ├── None → process::exit(1)    │            │
│     └── Some(path)                 │            │
│          │                         │            │
│          ▼                         │            │
│  ③ launch_daemon(sidecar, 7779)    │            │
│     yola-daemon start \            │            │
│       --port 7779 \                │            │
│       --foreground                 │            │
│     │                              │            │
│     ├── Err → process::exit(1)     │            │
│     └── Ok(child)                  │            │
│          │                         │            │
│          ▼                         │            │
│     State.child = Some(child)      │            │
│          │                         │            │
│          ▼                         │            │
│  ④ wait_for_daemon(7779, 15s, 500ms)           │
│     Loop: check → sleep 500ms → check → ...    │
│     │                                           │
│     ├── false (timeout)                         │
│     │   kill_daemon() → process::exit(1)        │
│     └── true                                    │
│          │                                      │
│          ▼                                      │
│     "Daemon listo." ◄────────────┐              │
│                                  │              │
│  ⑤ window.show()   ◄────────────┘              │
│     Ventana se hace visible.                    │
│     si-yola se carga en el webview.             │
│                                                 │
└──────────────────────┬──────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────┐
│ EVENT LOOP (Tauri)                              │
├─────────────────────────────────────────────────┤
│  Usuario interactúa con si-yola                 │
│  si-yola ↔ yola-daemon (localhost:7779)         │
│                       │                         │
│                       ▼                         │
│  Usuario cierra ventana                         │
│  WindowEvent::CloseRequested                    │
│  → kill_daemon(state)                           │
│    → child.kill()                               │
│    → child.wait()                               │
│    → state.child = None                         │
│  → Proceso termina                              │
└─────────────────────────────────────────────────┘
```

**Diagrama de tiempos (peor caso):**

| Paso | Duración |
|------|----------|
| `check_daemon_health()` inicial | ≤ 2s |
| `find_sidecar()` | < 1ms (stat syscall) |
| `launch_daemon()` | < 100ms (fork + exec) |
| `wait_for_daemon()` | ≤ 15s (30 intentos × 500ms) |
| `window.show()` | < 50ms |
| **Total peor caso** | **~17s** |

---

## 5. CROSS-PLATFORM

### 5.1 Diferencias por SO en el código

| Aspecto | Windows | macOS | Linux |
|---------|---------|-------|-------|
| `EXE_SUFFIX` | `.exe` | `""` | `""` |
| Sidecar binario | `yola-daemon.exe` | `yola-daemon` | `yola-daemon` |
| Señal kill | `TerminateProcess` | `SIGKILL` | `SIGKILL` |
| Ventana consola | Ocultada con `#![windows_subsystem = "windows"]` | N/A | N/A |
| Bundle primario | NSIS `.exe` | `.dmg` | `.AppImage` |
| Icono | `.ico` | `.icns` | `.png` |

### 5.2 Bundle targets por SO

| SO | Targets en tauri.conf.json | Artifact subido en CI |
|----|---------------------------|----------------------|
| **Windows** | `msi`, `nsis` | NSIS `.exe` (`bundle/nsis/*.exe`) |
| **macOS** | `dmg` | `.dmg` (`bundle/dmg/*.dmg`) |
| **Linux** | `deb`, `appimage` | `.AppImage` (`bundle/appimage/*.AppImage`) |

---

## 6. CONFIGURACIÓN — Resumen de constantes y defaults

| Parámetro | Valor | Dónde se define | Propósito |
|-----------|-------|-----------------|-----------|
| **Puerto daemon** | `7779` | `lib.rs` L24 `DAEMON_PORT` | Health check y conexión frontend |
| **Timeout health check** | `2s` | `lib.rs` L35 hardcodeado | Cada intento individual de `check_daemon_health()` |
| **Timeout arranque** | `15s` | `lib.rs` L25 `DAEMON_TIMEOUT_SECS` | Tiempo máximo total esperando al daemon |
| **Intervalo polling** | `500ms` | `lib.rs` L26 `DAEMON_POLL_MS` | Sleep entre health checks |
| **Ventana ancho** | `1200px` | `tauri.conf.json` L16 | Ancho inicial |
| **Ventana alto** | `800px` | `tauri.conf.json` L17 | Alto inicial |
| **Ventana visible** | `false` | `tauri.conf.json` L21 | Oculta hasta que daemon esté listo |
| **Título ventana** | `"YOLA"` | `tauri.conf.json` L15 | Barra de título nativa |
| **Bundle ID** | `com.yolabysayri.yola-desktop` | `tauri.conf.json` L5 | Identificador único |
| **Frontend dist** | `../../si-yola/dist` | `tauri.conf.json` L7 | Carpeta con build de SolidJS |
| **Dev URL** | `http://localhost:5173` | `tauri.conf.json` L8 | Vite dev server |

---

## 7. CI/CD — `.github/workflows/build.yml` (63 líneas)

### 7.1 Triggers

| Evento | Condición |
|--------|-----------|
| `push` | Rama `main` |
| `workflow_dispatch` | Manual desde GitHub UI |

### 7.2 Matrix de builds

| OS | Runner | Tauri target | Artifact |
|----|--------|-------------|----------|
| Windows | `windows-latest` | NSIS `.exe` | `yola-desktop-windows` |
| macOS | `macos-latest` | `.dmg` | `yola-desktop-macos` |
| Linux | `ubuntu-latest` | `.AppImage` | `yola-desktop-linux` |

`fail-fast: false` → si un SO falla, los otros continúan.

### 7.3 Pasos del job (orden secuencial)

| Paso | Qué hace | Detalle |
|------|----------|---------|
| 1. `actions/checkout@v4` | Clona el repo | — |
| 2. `Setup Bun` | Instala Bun latest | `oven-sh/setup-bun@v1` |
| 3. `bun install` | Instala dependencias JS | `@tauri-apps/cli`, icon utils |
| 4. **Build si-yola** | `cd ../si-yola && bun install && bun run build` | Genera `si-yola/dist/` que Tauri empaqueta como `frontendDist` |
| 5. `Setup Rust` | Instala Rust stable | `dtolnay/rust-toolchain@stable` |
| 6. **Linux deps** (solo Linux) | `apt-get install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf` | Dependencias nativas de WebKitGTK para Tauri en Linux |
| 7. `bun run tauri build` | Compila Rust + empaqueta app nativa | Genera binario + instaladores en `src-tauri/target/release/bundle/` |
| 8. Upload Windows | `actions/upload-artifact@v4` → `bundle/nsis/*.exe` | Solo si `runner.os == 'Windows'` |
| 9. Upload macOS | `actions/upload-artifact@v4` → `bundle/dmg/*.dmg` | Solo si `runner.os == 'macOS'` |
| 10. Upload Linux | `actions/upload-artifact@v4` → `bundle/appimage/*.AppImage` | Solo si `runner.os == 'Linux'` |

**Nota sobre el paso 4 (Build si-yola):** El workflow asume que `yola-desktop` y `si-yola` son directorios hermanos en el mismo repo (`../si-yola`). Esto no se refleja en `.gitignore` pero es la estructura del monorepo.

**Nota sobre artifacts:** El MSI de Windows y el `.deb` de Linux se generan pero no se suben como artifacts separados (solo NSIS, DMG y AppImage). Esto es una decisión de CI, no una limitación de Tauri.

---

## 8. ÁRBOL COMPLETO DEL PROYECTO

```
yola-desktop/
├── .github/workflows/build.yml          ← CI multi-OS
├── .gitignore                           ← node_modules/, src-tauri/target/
├── bun.lock                             ← Lockfile de Bun
├── package.json                         ← Scripts dev/build, deps iconos
├── README.md                            ← Documentación de usuario
├── CODE_MAP.md                          ← ESTE ARCHIVO
│
└── src-tauri/
    ├── build.rs                         ← tauri_build::build() → generate_context!()
    ├── Cargo.toml                       ← Dependencias Rust (tauri, reqwest, serde, tokio)
    ├── Cargo.lock                       ← Lockfile de Cargo
    ├── tauri.conf.json                  ← Config: ventana, bundle, frontend, iconos
    ├── yola-daemon.exe                  ← Sidecar precompilado para desarrollo Windows
    │
    ├── gen/schemas/                     ← Generado por tauri-build
    │   ├── acl-manifests.json           ← Access Control List manifests
    │   ├── capabilities.json            ← Capacidades de la app
    │   ├── desktop-schema.json          ← Schema de configuración desktop
    │   └── windows-schema.json          ← Schema de configuración de ventanas
    │
    ├── icons/                           ← Iconos en todos los formatos
    │   ├── 32x32.png
    │   ├── 128x128.png
    │   ├── 128x128@2x.png               ← Retina/HiDPI
    │   ├── 256x256.png
    │   ├── icon.icns                    ← macOS
    │   ├── icon.ico                     ← Windows
    │   └── icon.png                     ← Linux / genérico
    │
    └── src/
        ├── main.rs                      ← Entry point: fn main() → lib::run()
        └── lib.rs                       ← TODO el núcleo (214 líneas)
            ├── DaemonState              ← struct { child: Mutex<Option<Child>> }
            ├── DAEMON_PORT: 7779        ← const
            ├── DAEMON_TIMEOUT_SECS: 15  ← const
            ├── DAEMON_POLL_MS: 500      ← const
            ├── check_daemon_health()    ← GET /api/v1/health, timeout 2s, blocking
            ├── find_sidecar()           ← busca en 4 ubicaciones, retorna Option<PathBuf>
            ├── launch_daemon()          ← spawn "yola-daemon start --port N --foreground"
            ├── wait_for_daemon()        ← loop: check → sleep 500ms → max 15s
            ├── kill_daemon()            ← child.kill() + child.wait() + cleanup
            └── run()                    ← Tauri Builder: setup + on_window_event + manage + run
```

---

## 9. DECISIONES DE DISEÑO CLAVE

1. **Síncrono en setup:** Tauri v2 `setup()` no es async. `reqwest::blocking` y `std::thread::sleep` son necesarios. La UI no se bloquea porque la ventana está oculta durante setup.

2. **Sidecar en vez de embedded:** `yola-daemon` es un binario separado, no una librería linkeada. Ventajas: actualización independiente, separación de crashes, posibilidad de que otros clientes usen el daemon.

3. **`--foreground` es crítico:** Sin este flag, el daemon se demoniza y el `Child` handle no puede rastrearlo ni matarlo. El desktop es dueño exclusivo del ciclo de vida del daemon.

4. **Cuatro ubicaciones de sidecar:** Cubre desarrollo local, bundle de Tauri (`resources/`), build de Cargo (`target/release/`), y estructura de workspace (`../`). Robusto pero simple.

5. **Ventana oculta hasta ready:** Patrón común en apps que dependen de un backend. Evita pantalla en blanco o errores de conexión en el frontend.

6. **Cleanup en `CloseRequested`, no en `on_exit`:** Si la app crashea, el daemon puede quedar huérfano. Pero al menos en cierres normales se garantiza cleanup. `on_exit` no ayudaría más porque también depende de un shutdown graceful.

7. **Sin `beforeDevCommand`/`beforeBuildCommand`:** El build de si-yola se maneja explícitamente en CI. En desarrollo local, el dev debe iniciar Vite manualmente. Esto da control total sobre el orden de build.

---

## 10. POSIBLES MEJORAS (extraídas de TODOs y observaciones)

| ID | Descripción | Ubicación |
|----|-------------|-----------|
| TODO-1 | Reemplazar `process::exit(1)` en sidecar not found con diálogo nativo (`rfd::MessageDialog`) | `lib.rs` L147 |
| TODO-2 | Definir CSP restrictiva en vez de `null` | `tauri.conf.json` L25 |
| TODO-3 | Agregar `beforeDevCommand: "cd ../si-yola && bun run dev"` para desarrollo con un solo comando | `tauri.conf.json` L9 |
| TODO-4 | Agregar `beforeBuildCommand: "cd ../si-yola && bun run build"` para build con un solo comando | `tauri.conf.json` L10 |
| TODO-5 | Subir también `.msi` y `.deb` como artifacts en CI | `build.yml` L44-63 |
| TODO-6 | Agregar tests de integración para el flujo de arranque | `lib.rs` (sin tests actualmente) |
