# CopyForge — Plan consolidado v2

## Leyenda
- ✅ Hecho y verificado
- 🔄 En curso
- ⏳ Pendiente
- 🔗 Bloqueado por otro paso

## P0 — Correctitud y seguridad del motor

- **P0.1** ✅ Hecho — Symlinks reales (seguir vs recrear) + `FileResult::Symlinked` + `CopyEvent::Symlinked` + `Report{symlinks_created, symlinked}`. Scanner clasifica con `symlink_metadata` + detección de ciclos. Commit: `<hash>`.
- **P0.2** ✅ Hecho — `build_effective_dest` + 4 tests (file→new, file→dir, dir→new, dir→dir). API aditiva, `build_plan` mantiene firma. Commit: `<hash>`.
- **P0.3** ✅ Hecho — Concurrencia real: `acquire_owned()` movido DENTRO del `tokio::spawn` en `crates/core/src/engine.rs:58-68`. Paralelismo real sin deadlock (uso único del semáforo, N>=1, sin adquisición anidada, guard liberado al terminar `copy_task`). Commit: `3e33111`.
- **P0.4** ✅ Hecho — Error tipado: `FileResult::Failed{CopyError}` (interno, no breaking) en `crates/core/src/engine.rs`; `VerifyFailed` en Size/Hash y `Cancelled` en cancelación; `#[non_exhaustive]` en `CopyError`. Frontera pública (`CopyEvent::FileFailed`, `Report.errors`) sigue `String` vía `to_string()` — ver nota breaking abajo. Commit: `3e33111`.
- **P0.5** ⏳ Pendiente — Validación `src==dst` y `dst⊂src` (H-4): abortar antes de `build_plan` con `CopyError::InvalidDestination`. Afecta `crates/core/src/scanner.rs:11-14`, `apps/cli/src/runner.rs`, `apps/gui/src/app.rs`.
- **P0.6a** ✅ Hecho — `DurabilityPolicy { None, File, Full }` (default `File`; cambio documentado respecto al anterior equivalente a `None`): `apply_durability` con `sync_all` del temporal pre-rename para `File|Full`; `sync_parent_dir` post-rename solo en `Full` (Unix; Windows degrada a `File` con `warn` una sola vez por invocación vía `Arc<Once>` creado en `run()`). CLI `--durability <none|file|full>` (default `file`). 2 tests de contenido. Sin commitear todavía.
- **P0.6b** ⏳ Pendiente (siguiente paso) — Test de integración: con `Full` en Unix, verificar que el directorio padre se sincroniza.
- **P0.7** ✅ Hecho — Cap en `report.errors`: `CopyOptions.max_errors` (default 1000) + `Report.errors_truncated`; se conservan los primeros N, el resto solo cuenta. CLI imprime “N errores adicionales omitidos (total: M)”; GUI lo muestra junto a la lista. 2 tests + verificación real con 1005 fallos. Sin commitear todavía.
- **P0.8** ✅ Hecho — `src` symlink roto como raíz: el gate `!src.exists()` lo rechazaba con `SourceNotFound` aunque el enlace es recreable; además WalkDir falla con NotFound en profundidad 0. Gate con `symlink_metadata` + rama de tarea `Symlink` única en `crates/core/src/scanner.rs`. Test `build_plan_accepts_broken_symlink_src` + copia real verificada (`Enlaces creados: 1`). Commit: `8974688`.
- **P0.9** ✅ Hecho — Escape con `follow_symlinks=true` (default seguro + opt-in): `SymlinkEscapesSource{path,target}` si el target resuelto queda fuera de `src`; flag `allow_symlink_escape` en `CopyOptions` (default `false`), CLI `--allow-symlink-escape`, checkbox GUI “Permitir symlinks fuera de src”; cap `max_escape_bytes: Option<u64>` con `EscapeCapExceeded{limit,current}` (defensa en profundidad). 3 tests nuevos. Sin commitear todavía.

## P1 — Capa multiplataforma y robustez de temporales

- **P1.1** ⏳ Pendiente — Crear `crates/core/src/platform.rs` con: `symlink_file`, `symlink_dir`, `set_readonly`, `set_times`, `is_hidden`, `canonicalize_long`, `atomic_replace`, `sync_dir` (abrir dir con `FILE_FLAG_BACKUP_SEMANTICS` para implementar `DurabilityPolicy::Full` en Windows).
- **P1.1a** ✅ Hecho — Esqueleto `platform.rs` con `symlink_file/dir`, `set_readonly`, `is_hidden`, `canonicalize_long` (unix+windows, funciones libres) + `windows 0.61` solo con `Win32_Storage_FileSystem` + `Win32_Foundation` como `[target.'cfg(windows)'.dependencies]`. Engine sin `cfg` para crear symlinks; tests de scanner migrados a `platform::`. Quedan fuera (siguientes sub-pasos): `atomic_replace`, `sync_dir`, `set_times`. Rama Windows verificada por inspección (sin toolchain MSVC/MinGW aquí). 4 tests nuevos (24 en total). Sin commitear todavía.
- **P1.1b** ✅ Hecho — `atomic_replace` migrado a `platform.rs` (firmas verificadas contra el fuente vendored de `windows 0.61.3`: `Result<()>`, `PCWSTR::null()`, `HRESULT::from_win32`, `From<Error> for io::Error`; sin features nuevas). Engine sin `atomic_replace` inline. Tests unix + windows (análogo, corre en CI). CI: job Windows pasa a `cargo test --workspace`; tests que crean symlinks con `#[cfg_attr(windows, ignore)]` (privilegios; compilan en ambos, solo se ejecutan en Unix). Sin commitear todavía.
- **P1.2** ✅ Hecho (vía P1.1b) — `atomic_replace` Windows con `ReplaceFileW` + fallback a `MoveFileExW` si `ERROR_FILE_NOT_FOUND`; Unix mantiene `rename`; el `remove+rename` de `engine.rs` desaparece. Rama Windows verificada contra fuentes + CI (sin toolchain local).
- **P1.1c** ✅ Hecho — `sync_dir` en `platform.rs` (Unix `open+sync_all`; Windows `CreateFileW` con `FILE_FLAG_BACKUP_SEMANTICS` + `FlushFileBuffers` + `CloseHandle` siempre; +feature `Win32_Security` exigido por `CreateFileW`, detectado por E0432 en CI). Engine usa `platform::sync_dir`; eliminado `sync_parent_dir` + `warn_once`/`Arc<Once>` (ya sin usos). Doc de `Full` actualizado (funcional en ambas). Tests unix + windows. Sin commitear todavía.
- **P1.1c-fix3** ✅ Hecho — En Windows, `sync_dir` pasa a no-op documentado: `FlushFileBuffers` sobre handle de directorio falla con `E_ACCESSDENIED` en CI y NTFS ya journaliza los metadatos (no necesita fsync del directorio). Eliminados `CreateFileW`/`FlushFileBuffers`/`CloseHandle` y la feature `Win32_Security` (ya sin usos; verificado por grep). `Full` en Windows ≡ `File`. Test Windows cambiado a `sync_dir_returns_ok`.
- **P1.3** ✅ Hecho — `preserve_metadata` (renombrado desde `preserve_permissions`): mode Unix (existente), mtime+atime vía `filetime 0.2` post-rename (best-effort + debug), readonly en Windows si el origen lo era. Skips `IfNewer/IfDifferent` ahora emiten `Identical` (`Never` sigue `AlreadyExists`). 2 tests (mtime ±1s; segunda pasada `IfDifferent` no copia). Sin commitear todavía.
- **P1.4** ✅ Hecho — Temporales robustos (H-3): `NamedTempFile` con prefijo `.copyforge-tmp-` en el mismo dir que `dst` (mismo FS → rename atómico); `into_temp_path` + `keep()` tras éxito (sin `persist()`, conserva `ReplaceFileW`); auto-borrado por `Drop` en fallos; limpieza al inicio de `run()` (padres de tareas, 1 nivel, solo >1h para no tocar copias concurrentes). Sin commitear todavía.
- **P1.5** ✅ Hecho — Globs cross-platform: `GlobBuilder::literal_separator(true)` (estilo `.gitignore`; vive en `GlobBuilder`, no en `GlobSetBuilder`) + `normalize_for_glob` vía `components()` con `/`. 3 tests (target/**, normalize, symlink-a-dir). Sin commitear todavía.
- **P1.6** ✅ Hecho — Fallback de symlink en Windows (H-6): `platform::is_privilege_error` (`PermissionDenied` + crudos 5/1314, verificado en fuente `windows 0.61.3`); al fallar por privilegio se copia el contenido (vía follow) con `CopyEvent::SymlinkFallback` + `Report.symlinks_fallback`; render CLI/GUI. Sub-caso symlink-roto-a-dir: limitación conocida sin fix (el enlace no almacena su tipo; TODO(P1.6) en código). Sin commitear todavía.
  - Sub-caso: symlink roto apuntando a directorio en Windows → se recrea como file. Sin heurística fiable; documentar como limitación conocida y valorar fallback a copia del target o warning.
- **P1.7** ⏳ Pendiente — Validación insensible a mayúsculas (H-8): `validate_paths`/`validate_effective_paths` comparan byte a byte; en Windows/macOS `C:\Foo` y `c:\foo` son el mismo dir y bypassearían el chequeo. Normalizar caja al canonalizar en `platform.rs` (P1.1) cuando se cree.
- **P1.8** ⏳ Pendiente — Falso positivo menor (H-9): `src` symlink cuyo target es el `dst` (p. ej. `L→/real`, `dst=/real`) se rechaza como “same” aunque el efectivo (`/real/L`) sería válido. Workaround: pasar el `dst` explícito (`/real/L`). Mejora futura: no seguir el componente final symlink en `canonicalize_lossy` teniendo en cuenta `follow_symlinks`.

## P2 — Integración CLI/GUI

- Nota exit codes: `0` éxito / `1` fallos (incl. cancelación). Si más adelante
  se decide el `130` para SIGINT puro, hay que tocar `main.rs` (y `Report`
  para distinguir pre-cancelado de éxito vacío); por ahora se queda en `1`.

- **P2.1** ✅ Hecho — CLI: `ctrl_c()` → `cancel_token()` (watcher en `runner.rs`; engine ya cooperaba en spawn/tarea/bucle); limpieza de tmps por `Drop` (P1.4, verificado: 0 huérfanos tras Ctrl-C); `verbose` → `EnvFilter::try_new` con `RUST_LOG`/default `info` (helper puro `log_filter_from`); `exit_code_for` (fallos incl. cancel → 1); render `SymlinkFallback` ya existía (P1.6). 6 tests CLI. Ctrl-C manual verificado (exit 1, reporte “cancelado”). Semántica 0/1 documentada en `exit_code_for`; `130` para SIGINT puro queda como candidato P2.x (requiere indicador “cancelado” en `Report`, cambio en core). Sin commitear todavía.
- **P2.2** ⏳ Pendiente — GUI: exponer `overwrite/verify/jobs/exclude`; botón Cancelar conectado al token; fix `/0`; etiqueta "bytes" en vez de "archivos"; `Handle` global en vez de `Runtime` por copia; `rfd::AsyncFileDialog`. Afecta `apps/gui/src/app.rs:58,69-74,97,117,303-307`.
- **P2.3** ⏳ Pendiente — Progreso coherente con skips (H-5): `total_bytes` debe decrementarse con `SkipReason::{Identical,Excluded,NewerExists}`; el progreso llega a 100%. Afecta `engine.rs:34-36`, `apps/cli/src/ui.rs`, `apps/gui/src/app.rs:303-307`.
- **P2.4** ⏳ Pendiente — Semántica dir→dir idempotente (H-11): flag `--sync` o `--mirror` para copiar `src` → `dst` directamente sin anidar. Discutir semántica exacta (mirror vs merge) antes de implementar. Afecta `scanner.rs` (`build_effective_dest`), `apps/cli` args, `apps/gui` options.
- **P2.5** ⏳ Candidato — Cleanup de gates `#[cfg(unix)]` innecesarios (diagnóstico, sin tocar código): estos tests solo usan APIs portables (`tempfile`, `filetime`, engine, `std::fs`) y podrían correr en el CI Windows quitando el gate:
  - `engine.rs:734` `preserve_metadata_keeps_mtime` (mtime fijado vía `filetime`, tolerancia ±1s válida en NTFS).
  - `engine.rs:794` `failed_copy_leaves_no_temp` (plan manual + `read_dir`, 100% portable).
  - `engine.rs:828` `run_cleans_stale_temp_orphans` (`filetime` portable).
  - `engine.rs:849` `run_keeps_fresh_temp_files` (trivialmente portable).
  - NO candidatos (genuinamente específicos): `engine.rs:868` `symlink_recreate_still_works` (crear symlinks exige privilegio en Windows; opcional: pasar a `cfg_attr(windows, ignore)` como los 4 de scanner para cobertura de compilación); impls `platform.rs` con `std::os::unix::*`, `MetadataExt` y ramas de detección de ciclos en `scanner.rs`; `apps/cli/src/runner.rs:135` es un comentario, no un gate.

## P3 — Tests y robustez

- **P3.1** ⏳ Pendiente — `crates/core/tests/`: `plan.rs` (4 casos), `overwrite.rs` (Never/IfNewer/IfDifferent/Always + SkipReason::Identical), `verify.rs` (Size/Hash), `symlink.rs` (cfg(unix), follow true/false), `cancel.rs` (cancela a mitad + limpieza .tmp), `src_eq_dst.rs` (P0.5).
- **P3.2** ⏳ Pendiente — E2E en `apps/cli/tests/`: copia real de árbol pequeño con `assert_cmd`, verificación de `--overwrite` y `--verify`, fixture con `tempfile::TempDir`, cleanup automático.
- **P3.3** ⏳ Pendiente — Tests runtime Windows-específicos para `preserve_metadata` (readonly): copiar fichero readonly y verificar flag en destino; corre en CI windows-latest (en Linux esa rama es `cfg` y no se ejecuta).

## Hallazgos resueltos (integrados arriba)
- H-1 → P1.3 ✅ (mtime preservado; `Identical` en skips)
- H-2 → P0.6a (DurabilityPolicy; Full en Windows → P1.1) + P0.6b (test integración)
- H-3 → P1.4 ✅ (temporales con prefijo + limpieza con gate de antigüedad)
- H-4 → P0.5
- H-5 → P2.3
- H-6 → P1.6 ✅ (fallback a contenido + SymlinkFallback; sub-caso roto-a-dir: limitación conocida)
- H-7 → P0.7
- H-8 → P1.7 (validación case-insensitive en Windows/macOS)
- H-9 → P1.8 (falso positivo symlink-src→target-dir, con workaround)
- H-10 → P0.9 (escape follow-symlink fuera de src: default seguro + opt-in + cap)
- H-11 → P2.4 (semántica dir→dir no idempotente; flag --sync/--mirror pendiente de diseño)
- H-12 → P2 (candidato, sin item: lock `.copyforge-lock`; solo si hay evidencia de afectados)
- H-13 → P1.4-fix1 ✅ (keep() sobre path fantasma en Windows; refutada hipótesis de handles)

## Limitaciones conocidas (sin acción)
- TOCTOU validación→walk: el FS puede cambiar entre `validate_*` y el WalkDir/copia (p. ej. symlink sustituido tras validar). Sin locks no es cerrable del todo; la validación es defensa en profundidad, no garantía. No se actúa.
- `Path::starts_with` es por componentes (`/a/bb` NO está dentro de `/a/b`): verificado, sin falsos positivos por prefijo de string.
- Hardlinks src/dst hermanos: seguros (el rename del tmp rompe el link de `dst`, `src` intacto). Sin acción.

## Notas de proceso
- Cada paso se commitea al terminar: `git commit -m "P0.X: <descripción>"`.
- Al cerrar cada paso, actualizar este PLAN.md con el hash del commit.
- Si el agente se desvía, recordarle: "Estamos en P0.X, solo ese archivo".
- Un import no usado en `#[cfg(test)]` tumba el job (el CI compila los tests con `CARGO_BUILD_WARNINGS=deny`). Antes de cada commit, ejecutar `CARGO_BUILD_WARNINGS=deny cargo test --workspace --no-run` en local. Ojo: lo gateado por SO (`#[cfg(unix)]`) no se detecta en local; preferir tests portables y dejar el CI Windows como detector de lo específico.
- Nota breaking P0.3/P0.4: `FileResult` es privado (sin break). `CopyEvent`/`Report` mantienen `String` (sin break en repo). Único break intencional: `#[non_exhaustive]` en `CopyError` — externos con `match` exhaustivo deberán añadir wildcard.
- Detectado un bloque duplicado durante P1.6 (origen: edit propio de P1.6 en working tree, sin commitear; detectado y reparado en la misma sesión — ningún commit lo contuvo, verificado por conteo en `git show`). Era además error de sintaxis (`else` huérfano), así que `cargo check` lo habría rechazado igualmente. cargo check no detecta duplicados funcionales; conviene revisar diff manualmente cuando un paso grande añade bloques.
### P3 — Tests y robustez
- **P3.1** ⏳ Pendiente — `core/tests/`: plan (file/dir/exclude), overwrite `Never/IfNewer/IfDifferent`, verify `Size/Hash`, symlink Unix, cancel.
- **P3.2** ⏳ Pendiente — Integración end-to-end + fixtures con limpieza.

## 2. Hallazgos adicionales

- **H-1 (P1)** — `preserve_permissions` no preservaba `mtime` (recopiaba siempre en la segunda pasada). Resuelto en P1.3 con `preserve_metadata` + skips `Identical`. Estado: ✅ Hecho.
- **H-2 (P1)** — Sin `fsync`: `flush()` antes del rename no garantiza durabilidad. Resuelto vía `DurabilityPolicy` (P0.6a) + `sync_dir` en `platform.rs`: Unix hace `fsync` del padre; Windows es no-op documentado (NTFS journaliza metadatos, P1.1c-fix3). Estado: ✅ Hecho.
- **H-3 (P0)** — Temporales huérfanos (`*.copyforge.tmp`): resuelto en P1.4 con `NamedTempFile` (prefijo `.copyforge-tmp-`, auto-borrado) + limpieza al inicio de `run()` con gate >1h. Nota: el walk es anterior a la limpieza; un huérfano planificado falla de forma benigna (ver doc en `build_plan`). Estado: ✅ Hecho.
- **H-4 (P0)** — Sin validación `src==dst` ni `dst dentro de src`: riesgo de recursión explosiva/autosobrescritura. Afectado: `crates/core/src/scanner.rs:11-14`, `apps/cli/src/runner.rs`, `apps/gui/src/app.rs`. Estado: ⏳ Pendiente.
- **H-5 (P2)** — El progreso nunca llega a 100% con skips: `total_bytes` se calcula en el plan (`crates/core/src/engine.rs:34-36`) pero `Never/IfDifferent` luego saltan ficheros sin restar. Afectado además: `apps/cli/src/ui.rs`, `apps/gui/src/app.rs:303-307`. Estado: ⏳ Pendiente.
- **H-6 (P1)** — Symlink en Windows exige privilegio/Developer Mode. Resuelto en P1.6 con fallback a copiar contenido + `SymlinkFallback` (clasificador `PermissionDenied` + 5/1314). Sub-caso symlink-roto-a-dir: limitación conocida sin fix. Estado: ✅ Hecho.
- **H-7 (P0)** — `report.errors` sin cota: con millones de fallos puede agotar memoria. Capar (p.ej. 1000) + contador `errors_truncated`. Afectado: `crates/core/src/engine.rs:89-93`, `crates/core/src/events.rs`. Estado: ⏳ Pendiente.
- **H-11 (P2)** — Semántica dir→dir existente no es idempotente:
  - Causa: `build_effective_dest` anida `dst/src` cuando `dst` existe y es dir (semántica `cp -r`).
  - Efecto: `IfDifferent`/`IfNewer` no puede usarse para sincronización idempotente.
  - Candidato a fix: flag `--sync` o `--mirror` que cambie la semántica para copiar `src` → `dst` directamente sin anidar. Discutir semántica exacta (mirror vs merge) antes de implementar.
  - Afectado: `scanner.rs` (`build_effective_dest`), `apps/cli` args, `apps/gui` options.
  - Estado: ⏳ Pendiente.
- **H-12 (P2, candidato)** — Colisión de `cleanup_orphan_tmps` en copias concurrentes >1h.
  - Causa: el gate de 1h es heurístico. Copias largas pueden tener temporales legítimos con más de 1h de antigüedad.
  - Fix candidato: archivo `.copyforge-lock` en el directorio destino con PID + timestamp, y saltar el cleanup si hay un lock activo.
  - Umbral de activación: solo si hay evidencia de usuarios afectados.
- **H-13 (P1)** — `TempPath::keep()` tras `atomic_replace` falla en Windows.
  - Causa: en Unix `keep()` es no-op, pero en Windows ejecuta `SetFileAttributesW` sobre el path viejo del temporal, que ya no existe tras el rename → `ERROR_FILE_NOT_FOUND`. La hipótesis inicial (handles abiertos / `SHARING_VIOLATION`) se refutó: `into_temp_path()` cierra el handle interno y `drop(dst)` cierra el duplicado antes del rename.
  - Efecto: `FileResult::Failed` aunque la copia quedó correcta en disco (`files_copied=0` en los 3 tests de motor en CI Windows).
  - Fix aplicado (P1.4-fix1): eliminar `keep()`; tras rename exitoso no hay nada que conservar y el `Drop` del `TempPath` es no-op sobre el path movido.
  - Seguimiento: el flag `TEMPORARY` viaja con el rename al `dst`; si `Full` en Windows quiere quitarlo, hacerlo con `SetFileAttributesW` sobre el `dst`, no vía `keep()`.
  - Estado: ✅ Hecho.

## P4 — Migración de la GUI de egui a Tauri 2

Fase nueva. Decisión de producto: reescribir `apps/gui` con **Tauri 2** (backend Rust +
frontend React/TypeScript), en lugar de Iced. El motor (`crates/core`) no se toca en
ninguna parte de esta fase.

Contexto: P2.2a dejó `GuiOptions` encapsulando todo el estado editable de la GUI, lo que
convierte la migración en un mapeo casi 1:1 (`GuiOptions` → `State` de React, controles →
`Message`/setters). Se evaluaron dos alternativas con prototipos reales (Iced y Tauri 2).

**Decisión (final)**: Tauri 2. Iced se descarta porque la UI de TeraCopy (pestañas, tabla
de progreso, modales, densidad estilo Windows clásico) exige construir widgets de alto
nivel a mano; con HTML/CSS esa misma densidad cuesta mucho menos. El backend sigue siendo
el mismo motor Rust.

### Prototipo Iced — abandonado

- **P4.0** ❌ Abandonado — Prototipo mínimo de Iced aislado del workspace.
- **P4.1** ❌ Abandonado — Migrar `GuiOptions` → `State` de Iced.
- **P4.2** ❌ Abandonado — Migrar controles → `Message` + `update`.
  - **Nota**: Prototipo Iced validado (commits `d924f82`, `6eaf243`) pero descartado por
    la fricción de construir pestañas/tablas/modales a mano.
  - `apps/gui-iced` se conserva **congelado** en la rama `feature/gui-iced` como referencia.

### Migración Tauri 2 — activa

- **P4.0** ✅ Hecho — Prototipo Tauri 2 aislado, validado (`tauri dev` + `tauri build` OK).
  - **Ubicación original**: `/home/alejandro-pc/tauri-proto-copyforge` (fuera del repo).
  - **Stack**: React 19 + TypeScript + Vite; `identifier: com.copyforge.proto`.
  - **Maqueta TeraCopy-like**: filas Desde/Hacia con diálogos nativos
    (`@tauri-apps/plugin-dialog` + `tauri-plugin-dialog`), barra de progreso 64%,
    nombre del fichero en curso, botones Pausa/Pasar/Cancelar, barra de estado
    `1 / 4 · Total: 202.78 MB` y 3 pestañas (Lista de copia / Informe de errores /
    Opciones) que cambian el contenido mostrado.
  - **Validación**: ventana 800x600; diálogos rellenan el input; pestañas intercambian
    contenido; `tauri build` produce `.deb`, `.rpm` y `.AppImage`.
  - **Bloqueantes**: ninguno.
- **P4.1** ✅ Hecho — Integración en `apps/gui-tauri` (`3d870be`).
  - Se copia el prototipo al repo (sin `node_modules`/`target`/`dist`) y se aísla del
    workspace (`exclude = [..., "apps/gui-tauri/src-tauri"]`) para que
    `cargo check --workspace` no compile Tauri ni arrastre sus deps gráficas.
  - `copyforge-core` queda declarado por path en el `Cargo.toml` de Tauri (aún sin uso).
- **P4.1-fix** ✅ Hecho — Renombrado a `copyforge-gui-tauri` y README actualizado.
  - `package.json` + `src-tauri/Cargo.toml` → `copyforge-gui-tauri` (lib
    `copyforge_gui_tauri_lib`); `tauri.conf.json` → `productName: "CopyForge"`,
    `identifier: com.copyforge.app`.
  - `README.md` reescrito: sin referencias al prototipo antiguo ni a su ruta; documenta
    arranque, aislamiento del workspace y el workaround de `libpng`.
- **P4.2** ⏳ Pendiente — Conectar `GuiOptions`/`State` de React con comandos Tauri
  (`#[tauri::command]`) para leer/escribir opciones.
- **P4.3** ⏳ Pendiente — Conectar `CopyEngine` (spawn de copia + eventos `CopyEvent` →
  Tauri events → React).
- **P4.4** ⏳ Pendiente — Cancelación/Pausa reales.
- **P4.5** ⏳ Pendiente — Deprecar `apps/gui` (egui) y `apps/gui-iced`.

### Nota de entorno (Linux local)

Compilar Tauri falla con `libpng >= 1.4.0 could not be satisfied` en `pango-sys` porque
`/usr/local/lib/pkgconfig/libpng.pc` apunta a libpng12 (1.2.59) y ensombrece el del sistema
(1.6.57). Workaround no invasivo (no requiere tocar ficheros del sistema), solo para el
comando:

```sh
cd apps/gui-tauri
PKG_CONFIG_PATH=/usr/lib/x86_64-linux-gnu/pkgconfig npm run tauri dev   # o tauri build
```

En una máquina limpia (y en Windows) no hace falta.
