# CopyForge

> ⚠️ **Estado del proyecto: beta / RC.** CopyForge es usable en el día a día,
> pero sigue en desarrollo activo: CLI, API de `copyforge-core`, formato de
> reportes y GUI pueden cambiar entre releases sin garantía de compatibilidad.

Motor de copia de archivos multiplataforma estilo TeraCopy, escrito en Rust.
Copia archivos y carpetas con verificación de integridad, progreso en tiempo
real, políticas de sobrescritura y niveles de durabilidad configurables.

Proyecto organizado como workspace de Cargo con tres miembros:

| Miembro      | Paquete          | Descripción                                    |
|--------------|------------------|------------------------------------------------|
| `crates/core`| `copyforge-core` | Motor de copia (planificación + ejecución)     |
| `apps/cli`   | `copyforge`      | Interfaz de línea de comandos                  |
| `apps/gui`   | `copyforge-gui`  | Interfaz gráfica (egui/eframe)                 |

## Características

### Motor (`copyforge-core`)

- **Copia paralela**: varios archivos en vuelo a la vez (`--jobs`, por defecto 4),
  con búfer configurable por lectura (`--buffer-kib`, por defecto 256 KiB).
- **Planificación previa**: construye un plan (`CopyPlan`) con la lista exacta
  de tareas antes de copiar; el modo `--dry-run` lo muestra sin tocar nada.
- **Políticas de sobrescritura** (`--overwrite`):
  `always` | `never` | `if-newer` | `if-different` (tamaño + mtime).
- **Verificación post-copia** (`--verify`): `none` | `size` | `hash` (BLAKE3).
- **Durabilidad** (`--durability`): `none` | `file` (por defecto, `fsync` del
  temporal antes del rename) | `full` (además sincroniza el directorio padre;
  semántica TeraCopy).
- **Escritura atómica**: cada fichero se escribe en un temporal
  (`*.copyforge.tmp`) y se publica con rename atómico.
- **Symlinks**: por defecto se recrean como enlaces; con `--follow-symlinks`
  se copia el contenido resuelto. Por seguridad, seguir un enlace que apunte
  fuera del origen se bloquea salvo opt-in (`--allow-symlink-escape`), con
  límite opcional de bytes externos.
- **Exclusiones**: patrones glob relativos a la raíz, repetibles
  (`-x "*.log" -x "target/**"`).
- **Validación de rutas**: rechaza `origen == destino`, destino dentro del
  origen y destino efectivo dentro del origen (p. ej. copiar `/a/b` en `/a`).
  Resuelve `origen=archivo → destino=directorio` como `destino/nombre`.
- **Progreso y cancelación**: emite eventos (`Planned`, `FileStarted`,
  `Progress`, `FileCompleted`, `FileSkipped`, `FileFailed`, `Symlinked`,
  `Finished`) y soporta cancelación cooperativa.
- **Permisos**: preserva los permisos del origen (desactivable con
  `--no-preserve`).

### CLI (`copyforge`)

```text
Uso: copyforge [OPTIONS] <SOURCE> <DEST>

Opciones principales:
  -f, --overwrite <always|never|if-newer|if-different>  [defecto: always]
  -c, --verify <none|size|hash>                         [defecto: size]
      --durability <none|file|full>                     [defecto: file]
  -j, --jobs <N>          archivos en paralelo          [defecto: 4]
  -b, --buffer-kib <N>    búfer por lectura en KiB      [defecto: 256]
  -x, --exclude <GLOB>    excluir (repetible)
      --no-preserve       no preservar permisos
      --follow-symlinks   seguir enlaces en vez de recrearlos
      --allow-symlink-escape  permitir enlaces fuera del origen
      --fail-fast         detenerse al primer error
      --dry-run           mostrar el plan sin copiar
  -q, --quiet             solo resultado final
  -v                      verbosidad (-v, -vv)
```

Ejemplos:

```sh
# Copia básica con verificación por tamaño
copyforge ./docs ./backup/docs

# Solo archivos nuevos, con hash y 8 trabajos en paralelo
copyforge ./fotos ./backup/fotos -f if-newer -c hash -j 8

# Ver qué se haría, excluyendo temporales y logs
copyforge ./src ./dst --dry-run -x "target/**" -x "*.log"

# Máxima seguridad: hash + durabilidad total
copyforge ./datos ./backup/datos -c hash --durability full
```

### GUI (`copyforge-gui`)

- Campos de origen/destino con botones `Carpeta...` / `Archivo...`.
- Arrastrar y soltar archivos a la ventana (1 = origen, 2 = origen + destino).
- Barra de progreso, archivo actual, contadores y reporte final con errores.
- Casilla “Permitir symlinks fuera de src” (equivale a `--allow-symlink-escape`).
- Limitación conocida: en Wayland el arrastre no llega al programa; usar los
  botones de selección o pegar la ruta.

## Compilación y ejecución

Requisitos: Rust estable reciente (edition 2021) y Cargo.

```sh
# Compilar todo el workspace
cargo build --workspace

# CLI
cargo run -p copyforge -- --help
cargo run -p copyforge -- <origen> <destino> [opciones]

# GUI (requiere entorno gráfico)
cargo run -p copyforge-gui

# Tests
cargo test -p copyforge-core
cargo test --workspace

# Lints estrictos
cargo clippy --workspace --all-targets -- -D warnings
```

## Notas multiplataforma

- **Unix**: rename atómico real; `Full` sincroniza el directorio padre.
- **Windows**: el rename sobre existente se emula (borrar + renombrar);
  `Full` degrada a `File` con un aviso (una vez por ejecución) hasta contar
  con `platform.rs`. Crear symlinks puede requerir modo desarrollador o
  privilegios; sin ellos la copia del enlace falla con error claro.
- Los globs de exclusión son relativos a la raíz del origen.

## Licencia

MIT — ver el archivo `LICENSE`.

**Nota sobre patentes**: se ha revisado el código y no contiene algoritmos,
protocolos ni técnicas novedosas patentables ni patentadas; todo el motor se
basa en técnicas estándar (copia paralela por bloques, hash BLAKE3,
exclusión por globs, publicación atómica vía temporal + rename, `fsync` por
niveles). Por tanto no se requiere ninguna concesión de patente adicional y
la licencia MIT cubre el proyecto completo. (El manifiesto del workspace
declara `MIT OR Apache-2.0`, pero el único texto de licencia incluido en el
repo es el MIT de `LICENSE`, que es el que aplica.)
