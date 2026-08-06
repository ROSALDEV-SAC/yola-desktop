[![Release](https://img.shields.io/github/v/release/ROSALDEV-SAC/yola-desktop?color=6C5CE7)](https://github.com/ROSALDEV-SAC/yola-desktop/releases)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Download](https://img.shields.io/badge/download-latest-blue)](https://github.com/ROSALDEV-SAC/yola-desktop/releases/latest)

# yola-desktop -- YOLA Desktop App

![YOLA Desktop screenshot placeholder](docs/screenshot.png)

Wrapper Tauri v2 que empaqueta si-yola + yola-daemon como aplicacion de
escritorio nativa para Windows, Mac y Linux. Doble clic -> YOLA viva.

## Stack

- Tauri v2 (Rust backend + webview nativa)
- si-yola (SolidJS + Vite, frontend)
- yola-daemon (sidecar, lanzado como proceso hijo)

## Funcionamiento

Al abrir la aplicacion:

1. Chequea si el daemon responde en `localhost:7779` (`/api/v1/health`).
2. Si **no responde**: busca `yola-daemon(.exe)` en el mismo directorio que el
   ejecutable principal. Si no existe, muestra error nativo y cierra.
3. Si **existe**: lanza `yola-daemon start --port 7779 --foreground` como
   proceso hijo.
4. Espera en loop (maximo 15s, polling cada 500ms) hasta que el health check OK.
5. Muestra la ventana nativa con si-yola.
6. Al cerrar la ventana: **mata el proceso hijo** del daemon automaticamente.

## Estructura

```
src-tauri/          # Rust (Tauri v2): sidecar, lifecycle, ventana
../si-yola/dist/    # Frontend SolidJS compilado (tauri.conf.json -> frontendDist)
```

## Desarrollo

```bash
bun install
bun run tauri dev
```

Requiere Rust toolchain y Tauri CLI (`@tauri-apps/cli` v2).

## Build produccion

```bash
bun run tauri build
```

Genera:

- Windows: `.msi` (MSI installer) + `.exe` (NSIS installer)
- Mac: `.dmg`
- Linux: `.deb` + `.AppImage`
