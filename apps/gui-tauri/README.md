# copyforge-gui-tauri

GUI oficial de **CopyForge**, construida con **Tauri 2** (backend Rust + frontend
React/TypeScript + Vite). Sustituye a la GUI antigua de egui (`apps/gui`) y al
prototipo descartado de Iced (`apps/gui-iced`, congelado en la rama
`feature/gui-iced` como referencia).

Estado: **P4.1** — integrada en el repo y aislada del workspace. La UI es todavía una
maqueta estática estilo TeraCopy; la conexión con el motor de copia (`crates/core`) se
hace en P4.2/P4.3 (comandos Tauri ↔ `GuiOptions`, `CopyEngine` + eventos `CopyEvent`).
`copyforge-core` ya está declarada como dependencia por path, sin uso todavía.

## Arrancar

```sh
cd apps/gui-tauri
npm install
PKG_CONFIG_PATH=/usr/lib/x86_64-linux-gnu/pkgconfig npm run tauri dev
```

## Nota de entorno (Linux local)

En este equipo hay un `libpng.pc` obsoleto en `/usr/local/lib/pkgconfig/` (libpng12,
1.2.59) que ensombrece el del sistema (1.6.57) y hace fallar la compilación de
`pango-sys` con `libpng >= 1.4.0 could not be satisfied`. El workaround es no invasivo
(no toca ficheros del sistema) y se aplica solo al comando:

```sh
PKG_CONFIG_PATH=/usr/lib/x86_64-linux-gnu/pkgconfig npm run tauri dev   # o tauri build
```

En una máquina limpia (y en Windows) no hace falta.

## Aislamiento del workspace

El crate Rust de Tauri (`apps/gui-tauri/src-tauri`) está en la lista `exclude` del
`Cargo.toml` raíz, así que `cargo check --workspace` **no** lo compila ni arrastra sus
dependencias gráficas. Para compilarlo hay que entrar en su directorio o usar los
scripts de `npm` (`npm run tauri …`).

## Estructura

- `src/` — frontend React/TypeScript (maqueta TeraCopy-like: diálogos nativos,
  progreso, pestañas, opciones).
- `src-tauri/` — crate Rust de Tauri (comandos `#[tauri::command]`, plugins,
  `tauri.conf.json`).
- `dist/` — build del frontend (generado por `vite build`, ignorado por git).

## Build

```sh
PKG_CONFIG_PATH=/usr/lib/x86_64-linux-gnu/pkgconfig npm run tauri build
```

Genera `.deb`, `.rpm` y `.AppImage` en `src-tauri/target/release/bundle/`.
