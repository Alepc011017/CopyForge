# tauri-proto-copyforge

Prototipo **desechable** de CopyForge con Tauri 2, para evaluar aspecto visual
y experiencia de desarrollo frente a la alternativa Iced (`apps/gui-iced`).

- Una sola pantalla estática estilo TeraCopy (progreso 64%, tabla dummy,
  pestañas, opciones). Sin lógica real.
- Los botones «Carpeta…»/«Archivo…» abren diálogos nativos (plugin
  `@tauri-apps/plugin-dialog`) y rellenan el input. Nada más.
- **NO** conecta al motor de copia (`crates/core`). Eso sería un paso
  posterior solo si el prototipo convence.

## Arrancar

```sh
npm install
npm run tauri dev
```

### Nota de entorno (Linux de este equipo)

Hay un `libpng.pc` obsoleto en `/usr/local/lib/pkgconfig/` (libpng12, 1.2.59) que
ensombrece el del sistema (1.6.57) y hace fallar la compilación de `pango-sys`
con `libpng >= 1.4.0 could not be satisfied`. Solución no invasiva: forzar la
ruta del sistema solo para el comando:

```sh
PKG_CONFIG_PATH=/usr/lib/x86_64-linux-gnu/pkgconfig npm run tauri dev
```

(Lo mismo para `npm run tauri build`.) En una máquina limpia no hace falta.

## Borrar

```sh
cd /home/alejandro-pc
rm -rf tauri-proto-copyforge
git branch -D feature/gui-tauri   # solo si se creó esa rama (este proto vive fuera del repo)
```
