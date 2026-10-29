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
- **P0.5** ✅ Hecho — `4e3a756` — Validación `src==dst` y contención mutua (H-4) en `build_plan`.
  - `scanner::validate_paths` compara **por componentes** tras `canonicalize_lossy`
    (tolera `dst` inexistente) y devuelve `CopyError::InvalidDestination`:
    `src==dst` → "el origen y el destino son la misma ruta"; `dst⊂src` o `src⊂dst` →
    "origen y destino se contienen mutuamente". `validate_effective_paths` queda como
    defensa en profundidad (destino efectivo == `src` o dentro de `src`).
  - Mensajes en español, con la misma redacción que mostraba `validate_move_pair` en la GUI.
  - `apps/gui-tauri`: se elimina `validate_move_pair` (duplicaba la regla); `start_transfer`
    propaga tal cual el error de `build_plan` (también en modo `copy`).
  - Tests: `validate_rejects_same_src_dst`, `validate_rejects_dst_inside_src`,
    `validate_rejects_src_inside_dst` (antes permitía) y
    `build_plan_rejects_invalid_destination_pairs` en core (40); el equivalente del crate
    Tauri pasa a llamar a `build_plan` (siguen 15). CLI verificado: los tres casos dan
    `destino inválido: …` con exit 1 y sin tocar nada; la copia disjunta sigue OK.
  - Trade-off documentado: al rechazar también `src⊂dst` se pierde `copy <ruta> --to <abuelo>`
    (destino efectivo distinto de `src`) aunque no era peligroso; es la política que ya
    aplicaba la GUI. Para relajarlo basta con dejar solo `dst.starts_with(src)` en
    `validate_paths`: los casos realmente autodestructivos los cubre
    `validate_effective_paths`.
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
- Si `git status` muestra modificaciones sin commitear en archivos ya commiteados, revisar `git diff` antes de verificar: auto-saves de editores pueden revertir cambios en curso (visto durante P4.6b).
### P3 — Tests y robustez
- **P3.1** ⏳ Pendiente — `core/tests/`: plan (file/dir/exclude), overwrite `Never/IfNewer/IfDifferent`, verify `Size/Hash`, symlink Unix, cancel.
- **P3.2** ⏳ Pendiente — Integración end-to-end + fixtures con limpieza.

## 2. Hallazgos adicionales

- **H-1 (P1)** — `preserve_permissions` no preservaba `mtime` (recopiaba siempre en la segunda pasada). Resuelto en P1.3 con `preserve_metadata` + skips `Identical`. Estado: ✅ Hecho.
- **H-2 (P1)** — Sin `fsync`: `flush()` antes del rename no garantiza durabilidad. Resuelto vía `DurabilityPolicy` (P0.6a) + `sync_dir` en `platform.rs`: Unix hace `fsync` del padre; Windows es no-op documentado (NTFS journaliza metadatos, P1.1c-fix3). Estado: ✅ Hecho.
- **H-3 (P0)** — Temporales huérfanos (`*.copyforge.tmp`): resuelto en P1.4 con `NamedTempFile` (prefijo `.copyforge-tmp-`, auto-borrado) + limpieza al inicio de `run()` con gate >1h. Nota: el walk es anterior a la limpieza; un huérfano planificado falla de forma benigna (ver doc en `build_plan`). Estado: ✅ Hecho.
- **H-4 (P0)** — Sin validación `src==dst` ni contención origen/destino: riesgo de recursión explosiva/autosobrescritura. Resuelto en P0.5 con `scanner::validate_paths` (por componentes, `canonicalize_lossy`) + `validate_effective_paths` como defensa en profundidad; el error se propaga a CLI y GUI desde `build_plan`. Estado: ✅ Hecho (`4e3a756`).
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
- **P4.2** ✅ Hecho — `GuiOptions` de React ↔ comandos Tauri + UI de monitorización moderna.
  - Commits de la fase: `431c40d` (get/set_options + estado React), `ad826c3` (UI
    moderna + CI propio), `ce0f49d` (aviso cuando no hay IPC), `bb83cac` (fix invoke +
    dos columnas con disco), `285d1dd` (barra de archivo a ancho completo, ventana
    dinámica, barra de título propia y modo oscuro), `004f5b2` (ventana compacta, icono
    integrado, `Fichero: n / N` en cabecera y 16 filas con scroll) y `97dc5c3` (textos
    en español, botón `Saltar` y ventana 800x250).
  - **Decisión de diseño**: la GUI monitoriza una **cola de tareas** (la pestaña
    «Lista de copia»), no una ventana por copia. Un único proceso con una ventana
    compacta que crece al abrir el panel de detalles; el estado de la copia en curso
    vive en `CopyState` (P4.3), no en ventanas nuevas.
  - Backend: DTO `GuiOptions` (`Serialize`/`Deserialize`) con enums espejo
    (`OverwriteDto`/`VerifyDto`/`DurabilityDto`); `From`/`Into` con `CopyOptions`
    (core no se toca: no tiene serde). Comandos `get_options`/`set_options`, que
    registran con `tracing::info!` (deps `tracing` + `tracing-subscriber` en el
    backend). `exclude` (una línea) → `exclusion: Vec<String>`.
  - Frontend: interfaz TS, `get_options` al montar (error visible en la barra de
    estado) y `set_options` en cada cambio de control de la pestaña Opciones.
  - UI de monitorización (no formulario): cabecera con el icono 64x64
    (`icon/CopyForge.svg`) y Origen/Destino como texto de solo lectura (sin
    inputs ni diálogos de carpeta); dos barras de progreso con el texto dentro
    (global 40px con % y bytes; por fichero 28px con nombre y %); acciones a la
    derecha; barra de estado; y un menú desplegable (☰) con Lista de copia /
    Informe de errores / Opciones (aquí viven los controles de `GuiOptions`).
  - CSS reescrito (sin `#ece9d8`/Tahoma/outset): fondo `#f3f4f6`, bordes
    `#d1d5db` con radio 6px, progreso azul `#2563eb` (global) y verde `#10b981`
    (fichero en curso), transiciones de anchura/color.
  - `vite.config.ts` autoriza (`server.fs.allow`) la raíz del repo para poder
    importar el icono, que vive fuera de `apps/gui-tauri`.
  - CI propio en `.github/workflows/tauri-ci.yml` (push/PR a `feature/gui-tauri`
    y `main`): el `ci.yml` no compila la GUI porque está fuera del workspace;
    este workflow hace `npm ci` + `npm run build` y `cargo check` +
    `cargo test` sobre `apps/gui-tauri/src-tauri`, con deps de sistema de Tauri
    en Linux y caché de Cargo/target (`Swatinem/rust-cache`).
  - 7 tests en el crate de Tauri (mapeo a defaults de core, parseo de `exclude`,
    round-trip, formato wire del payload de React, alturas de ventana).
- **P4.3** 🔄 En curso — Conectar `CopyEngine` (spawn de copia + eventos `CopyEvent` →
  Tauri events → React).
- **P4.3-fix** ✅ Hecho — `b1cfeef` — `Finished` completo en el DTO.
  - `CopyEventDto::Finished` añade `errors_truncated: usize` (P0.7) y
    `symlinks_fallback: Vec<(String, String)>` (P1.6). El enum pasa a
    `Serialize + Deserialize` para el test de round-trip. El mapeo usa
    `report.symlinks_fallback.iter().map(|(src, dst)| (path_str(src), path_str(dst)))`
    (tipo confirmado: `Vec<(PathBuf, PathBuf)>`, subconjunto de
    `files_copied + dirs_created`, no categoría disjunta).
  - Frontend: el payload `finished` declara los dos campos; el aviso muestra
    “N errores adicionales omitidos (total: M)” solo si `errors_truncated > 0`.
    `symlinks_fallback` viaja en el contrato pero su render puede esperar.
  - Tests: `finished_dto_keeps_every_report_field` (un `Report` con todos los
    campos no-default ⇒ el DTO no pierde ninguno; detector limitado a los campos
    listados) y `finished_dto_round_trips_through_json` (`type":"finished"` +
    campos snake_case).
  - Verificación: `CARGO_BUILD_WARNINGS=deny cargo test --no-run && cargo test`
    dentro de `apps/gui-tauri/src-tauri` (el crate está excluido del workspace,
    la puerta de la raíz no lo cubre) + `npm run build`.

- **P4.3-fix2** ✅ Hecho — `80fd3dd` — Arranque único, liberación del token
  y `copy-cancelled`.
  - `CopyState.cancel` pasa a `Arc<Mutex<Option<CancellationToken>>>` y
    `CopyState` a `#[derive(Clone, Default)]`. Motivo: el guard `ReleaseToken`
    vive dentro de un `tokio::spawn` (exige `'static`) y `State<'_, T>` no es
    `Clone` en Tauri 2.
  - `start_copy` rechaza un segundo arranque con
    `Err("ya hay una copia en curso")` mientras haya token en el slot. El token
    se libera vía `ReleaseToken::drop` **después** de que el reenviador drene el
    canal (`oneshot`), evitando un `Finished` obsoleto sobre una copia nueva.
  - `cancel_copy` añade `app: tauri::AppHandle`, clona el `CancellationToken`
    dentro del lock y hace `cancel()` + `emit("copy-cancelled", ())` fuera (no
    se emite con el `MutexGuard` tomado). El frontend resetea `cancelledRef` en
    `startCopy` (tras el guard de backend/running) y consume el flag en el caso
    `finished`, mostrando el texto de cancelado.
  - Verificación manual (sin unit test viable, requiere `AppHandle`/`State`):
    dos `start_copy` seguidos ⇒ el segundo devuelve `Err`; Cancelar a mitad ⇒
    `copy-cancelled` + `finished` con texto de cancelado; tercer `start_copy`
    tras terminar funciona sin arrastrar el flag. `npm run build` OK.
  - Nota: `ReleaseToken` libera el slot ante un panic del motor, pero el perfil
    release usa `panic = "abort"`, así que solo protege en debug. Ventana de
    drenado (t0→t3, ~µs): `cancel_copy` ahí es un no-op y un `start_copy`
    recibe “ya hay una copia en curso” (reintento OK). Un cuelgue del reenviador
    dejaría el slot ocupado (mismo comportamiento que sin guard; el `break` del
    loop cubre el caso normal).
  - **Pitfall (deadlock de drenado)**: el spawn del motor retenía `engine` (y su
    `events_tx`) mientras esperaba al consumidor (`drained_rx`); como el canal no
    se cierra hasta soltar `events_tx`, la espera era circular y el token nunca
    se liberaba. Regla general: **no retener el productor mientras se espera al
    consumidor** — `drop(engine)` antes de `drained_rx.await`.
- **P4.4** 🔄 En curso — Pausa/reanudar reales (`b1d635e`).
  - Motor: `CopyEngine::with_pause(options, tx, paused)` + flag `Arc<AtomicBool>`
    consultado en la puerta entre ficheros y en el bucle de `copy_file` (entre
    chunks); la cancelación sigue funcionando en pausa. `COPYFORGE_THROTTLE_US`
    (solo `debug_assertions`) da margen para probar la pausa en ventana.
  - Tauri: `pause_copy(state, paused)`; el flag vive en `CopyState.paused`.
  - Frontend: botón Pausa ↔ Reanudar; Saltar sigue deshabilitado (requiere un
    handle por tarea). `cancelled: bool` en `Report` (H-D/A) sigue diferida a
    P2.1/130.
  - Tests: `pause_blocks_and_resume_copies` (pre-pausado ⇒ sin avance; reanudar ⇒
    `files_copied > 0`) + smoke en ventana con throttle.
  - Gotcha de desarrollo (React 18/19 StrictMode): en `npm run dev` los efectos de
    montaje se ejecutan **dos veces** (setup → cleanup → setup). En P4.6b, el efecto
    de escucha (`listen("copy-request")` + `drain_transfers` + `start_transfer`) se
    montaría dos veces y el segundo `start_transfer` recibiría `Err("ya hay una copia
    en curso")`. Fix: guard con ref (`drainedRef.current`) en el efecto de montaje.
    En producción no aplica; en dev solo ensucia el log. Anotado, sin implementar.
- **P4.4-fix** ✅ Hecho — `1be3900` — `tokio::time::sleep` en vez de busy-wait.
  - `wait()` (`yield_now` + `spin_loop`) se sustituye por `tokio::time::sleep` en los
    tres sitios (puerta entre ficheros, gate entre chunks y throttle de depuración).
    Se añade el feature `time` a tokio en `crates/core`.
  - Motivo: el busy-wait cooperativo con 8 tareas pausadas y 8 workers subía la CPU
    al ~100 % durante la pausa (ventilador/batería en una GUI de escritorio).
    Funcionalmente correcto, pero anti-patrón.
  - Verificación: 38 tests de core; smoke en ventana con `COPYFORGE_THROTTLE_US`:
    pausa 5 s ⇒ no arrancan más ficheros; reanudar ⇒ 4 ficheros, 0 fallos.
- **P4.6a** ✅ Hecho — `a6ef65c` (+ `eb04e0a`) — CLI en la GUI.
  - `src/cli.rs`: `LaunchArgs` + `Command::Copy/Move(TransferArgs)` (clap);
    `From<TransferArgs> for GuiOptions`; re-exportado como `LaunchArgs`.
  - `main.rs`: `run(LaunchArgs::parse())`; `run(args)` encola la transferencia inicial en
    `PendingCommands` (la primera instancia no dispara el callback de single-instance).
  - `tauri_plugin_single_instance`: una segunda instancia re-parsea su argv, hace `push` y
    emite `copy-request`; el frontend la recoge con `drain_transfers`.
  - `start_transfer(app, state, srcs, dst, options, mode)` (sustituye a `start_copy`):
    concatena los planes con `CopyPlan::extend` (una sola cola) y, en modo `move`, borra
    los orígenes si `files_failed == 0 && files_skipped == 0`, emitiendo `transfer-moved`.
  - Validación ad-hoc de `move` (bloqueante, `validate_move_pair`): rechaza igualdad
    canónica y contención por componentes en ambos sentidos.
  - Frontend: efecto de montaje con guard `drainedRef` (StrictMode) que drena y arranca cada
    transferencia; `startCopy` pasa `srcs: [src], mode: "copy"`; escucha `transfer-moved`.
  - Tests: parseo de la CLI (copy/move, defaults, valores desconocidos), `validate_move_pair`,
    `PendingCommands` (push/drain) y `CopyPlan::extend` en core.
  - Depende de P4.4 ✅. El check ad-hoc `validate_move_pair` se retiró en P0.5 (la regla vive ya en `build_plan`).
  - Verificación de cierre (V1–V3, las tres bloqueantes):
    - **V1 single-instance**: 2.ª instancia con args ⇒ `exit 0` en 0,107 s y sin ventana
      (0 `get_options`); la 1.ª sigue viva y ejecuta la tarea
      (`start_transfer ... mode=Copy files_copied=1`).
    - **V2 move + `--overwrite never` con destino existente**: `files_skipped=1`,
      `files_failed=0`, origen intacto con su contenido y destino sin cambios
      (no se borra nada si hay saltados).
    - **V3 `validate_move_pair`**: `src==dst` ⇒ `"el origen y el destino son la misma
      ruta"`; `dst⊂src` y `src⊂dst` ⇒ `"origen y destino se contienen mutuamente"`;
      rutas disjuntas ⇒ `Ok`; no toca el disco.
- **P4.6a-fix** ✅ Hecho — `eb04e0a` — Drenado al montar y mensaje propio para `src==dst`.
  - El efecto de montaje registra los listeners **siempre**; solo el primer drenado queda
    tras el guard `drainedRef`. Con el guard envolviendo todo el efecto, el segundo montaje
    de StrictMode (setup → cleanup → setup) se saltaba el `listen("copy-request")` y las
    tareas de instancias posteriores no llegaban a la UI (la 1.ª sí, vía `run(args)`).
  - `validate_move_pair` canoniza **antes** de comprobar contención, para que `src==dst`
    devuelva "el origen y el destino son la misma ruta" en vez del mensaje genérico.
  - Verificado con V1 (2.ª instancia ⇒ `exit 0` en 0,107 s y tarea ejecutada por la 1.ª)
    y V3 (los tres rechazos con su mensaje).
- **P4.6b** ✅ Hecho — `7b3d961` — Extensión de Nautilus (`integrations/nautilus/`).
  - `copyforge_extension.py` (274 líneas; antes 685): menús planos de fondo
    (`Pegar con CopyForge` → `copy`/`move` según el portapapeles, `Mover aquí con CopyForge`)
    y de selección (`Copiar con CopyForge` → `copy`, `Mover con CopyForge` → `cut`).
  - Se elimina el terminal (`_write_temp_script`, `_build_terminal_command`,
    `_find_terminal`, `TERMINAL_CANDIDATES`, `OPEN_TERMINAL`,
    `KEEP_TERMINAL_ON_SUCCESS`), el `flock` (`_lock_path_for`, `SERIALIZE_OPERATIONS`),
    los colores, `STOP_ON_ERROR`, `MOVE_VERIFY`, `CLEAN_STALE_TEMPS`/`STALE_TEMP_AGE_MIN`,
    `LOG_MAX_BYTES` y `DEBUG_MODE`. `_launch(mode, dst, srcs)` lanza
    `copyforge-gui-tauri <mode> <src...> --to <dst> --overwrite always --verify size`
    con `Popen(..., start_new_session=True)`; la GUI encola y el motor de `crates/core`
    ejecuta (sin tocar `crates/core`).
  - **Bug de `cut` corregido**: se intenta `x-special/gnome-copied-files` **primero**
    (`wl-paste`/`xclip`), el único tipo que distingue copiar de cortar; solo si falla se
    usa `Gdk.FileList` y, después, `text/uri-list` (con `"copy"` asumido, porque no
    llevan la acción).
  - `copyforge_ext_logic.py`: lógica pura **sin `gi`** (`parse_clipboard`,
    `filter_sources`, `gui_bin`, comandos y payload del portapapeles) para testearla sin
    Nautilus.
  - `_notify_error`: `zenity --error` si está instalado; siempre stderr y
    `~/.local/state/copyforge/nautilus.log` (`COPYFORGE_LOG` lo cambia).
  - `install.sh`: copia a `~/.local/share/nautilus-python/extensions`, detecta
    `nautilus-python` (con pistas por distro), avisa de `wl-clipboard`/`xclip`/`zenity`
    y soporta `--uninstall`.
  - Tests: `python3 -m unittest discover integrations/nautilus/tests` ⇒ 21 OK, sin `gi`.
  - Smoke real en esta máquina (GNOME nautilus 50.2.2, typelib Nautilus 4.1, pygobject
    3.56.2, `wl-clipboard` 2.x + zenity): Nautilus carga la extensión
    (`init: gui=... portapapeles=['wl-paste', ...]` en el log) y, con `gi` real y objetos
    simulados, los cuatro menús salen con su etiqueta; `cut` ⇒ `mode=Move` con origen
    borrado tras copia limpia; `copy` ⇒ `mode=Copy` con origen intacto;
    `_on_copy`/`_on_cut` escriben `copy`/`cut` + `file://…` en el portapapeles.
    No se pudo pulsar el menú en la ventana (el agente no interactúa con el SO), así que
    la comprobación es de carga real + código de menús/callbacks, no de clic visual.
  - Pendiente de entorno: el binario de la GUI debe estar en `PATH` o en
    `COPYFORGE_GUI_BIN` del **entorno de la sesión** (Nautilus no hereda el shell). En
    esta máquina no está en `PATH` (`init: gui=None`); el smoke se hizo exportando la
    variable. Documentado en el README.
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
