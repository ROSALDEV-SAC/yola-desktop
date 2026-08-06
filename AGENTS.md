# AGENTS.md — yola-desktop

Eres un agente de YOLA trabajando en este repositorio.

## Stack
- Shell: Tauri v2 (Rust edition 2021)
- Frontend: si-yola (SolidJS + Vite) empaquetado como recurso estático
- Backend Rust: tokio 1, reqwest 0.12 (blocking), serde
- Package manager: Bun (solo para scripts de íconos)
- CI: GitHub Actions (ubuntu, macos, windows-latest)

## Estructura
- `src-tauri/src/main.rs` — Bootstrap (6 líneas), oculta consola en Windows release
- `src-tauri/src/lib.rs` — Lógica real: health check del daemon, spawn, comandos Tauri
- `src-tauri/tauri.conf.json` — Config de ventana, URL, permisos
- `src-tauri/icons/` — Íconos por plataforma
- `src-tauri/yola-daemon.exe` — Binario del daemon empaquetado (Windows)
- `package.json` — Solo scripts Tauri CLI + utilidades de íconos (png-to-icns, sharp)

## Cómo buildear
```
bun run build
```

## Cómo testear
No hay test suite propia. El health check del daemon se verifica en CI (GitHub Actions).
Para desarrollo:
```
bun run dev
```

## Reglas
- El frontend (si-yola) vive EN OTRO REPO — este repo solo lo empaqueta, no lo edita
- El binario del daemon (`yola-daemon.exe`) se obtiene de yola-releases, no se compila aquí
- `src-tauri/tauri.conf.json` define los permisos del sistema — no agregues permissions innecesarios
- Nunca edites `src-tauri/gen/` a mano (generado por Tauri)

## Dónde tocar
- ¿Cambiar URL del frontend? → `src-tauri/tauri.conf.json`
- ¿Cambiar lógica de spawn del daemon? → `src-tauri/src/lib.rs`
- ¿Nuevo ícono? → `src-tauri/icons/` + scripts en package.json
