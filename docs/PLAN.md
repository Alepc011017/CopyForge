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
- **P0.6** ⏳ Pendiente — Integridad: `sync_all()` en el archivo temporal antes del rename, y `sync_all()` del directorio padre tras el rename en Unix (H-2). Afecta `crates/core/src/engine.rs:350-355`.
- **P0.7** ⏳ Pendiente — Cap en `report.errors`: limitar a 1000 entradas + campo `errors_truncated: usize` (H-7). Afecta `crates/core/src/engine.rs:89-93`, `crates/core/src/events.rs`.

## P1 — Capa multiplataforma y robustez de temporales

- **P1.1** ⏳ Pendiente — Crear `crates/core/src/platform.rs` con: `symlink_file`, `symlink_dir`, `set_readonly`, `set_times`, `is_hidden`, `canonicalize_long`, `atomic_replace`.
- **P1.2** ⏳ Pendiente — `atomic_replace` Windows con `ReplaceFileW` + fallback a `MoveFileExW` si `ERROR_FILE_NOT_FOUND`. En Unix mantiene `rename`. Sustituye `remove+rename` actual en `engine.rs:419-430`.
- **P1.3** ⏳ Pendiente — `preserve_metadata` completo: `mtime` + `mode` en Unix; `readonly` + `mtime` en Windows. Incluye H-1 (mtime roto en `IfDifferent`/`IfNewer`). Afecta `engine.rs:380-384`.
- **P1.4** ⏳ Pendiente — Temporales robustos (H-3): `tempfile::NamedTempFile` en el mismo directorio + limpieza de huérfanos `*.copyforge.tmp` al inicio. Excluir el patrón en `scanner.rs:118-122`.
- **P1.5** ⏳ Pendiente — Globs cross-platform: `GlobBuilder::literal_separator(true)` + normalizar `/` antes de matchear. Afecta `scanner.rs:103-116`.
- **P1.6** ⏳ Pendiente — Fallback de symlink en Windows (H-6): si `symlink_file`/`symlink_dir` devuelve `EACCES`, copiar contenido + emitir `CopyEvent::SymlinkFallback` con warning. Afecta `engine.rs:212-223`.

## P2 — Integración CLI/GUI

- **P2.1** ⏳ Pendiente — CLI: `tokio::signal::ctrl_c()` → `cancel_token().cancel()`; `verbose` → `EnvFilter` con `RUST_LOG`. Limpieza de `.tmp` al cancelar. Afecta `apps/cli/src/runner.rs:68-71`.
- **P2.2** ⏳ Pendiente — GUI: exponer `overwrite/verify/jobs/exclude`; botón Cancelar conectado al token; fix `/0`; etiqueta "bytes" en vez de "archivos"; `Handle` global en vez de `Runtime` por copia; `rfd::AsyncFileDialog`. Afecta `apps/gui/src/app.rs:58,69-74,97,117,303-307`.
- **P2.3** ⏳ Pendiente — Progreso coherente con skips (H-5): `total_bytes` debe decrementarse con `SkipReason::{Identical,Excluded,NewerExists}`; el progreso llega a 100%. Afecta `engine.rs:34-36`, `apps/cli/src/ui.rs`, `apps/gui/src/app.rs:303-307`.

## P3 — Tests y robustez

- **P3.1** ⏳ Pendiente — `crates/core/tests/`: `plan.rs` (4 casos), `overwrite.rs` (Never/IfNewer/IfDifferent/Always + SkipReason::Identical), `verify.rs` (Size/Hash), `symlink.rs` (cfg(unix), follow true/false), `cancel.rs` (cancela a mitad + limpieza .tmp), `src_eq_dst.rs` (P0.5).
- **P3.2** ⏳ Pendiente — E2E en `apps/cli/tests/`: copia real de árbol pequeño con `assert_cmd`, verificación de `--overwrite` y `--verify`, fixture con `tempfile::TempDir`, cleanup automático.

## Hallazgos resueltos (integrados arriba)
- H-1 → P1.3
- H-2 → P0.6
- H-3 → P1.4
- H-4 → P0.5
- H-5 → P2.3
- H-6 → P1.6
- H-7 → P0.7

## Notas de proceso
- Cada paso se commitea al terminar: `git commit -m "P0.X: <descripción>"`.
- Al cerrar cada paso, actualizar este PLAN.md con el hash del commit.
- Si el agente se desvía, recordarle: "Estamos en P0.X, solo ese archivo".
- Nota breaking P0.3/P0.4: `FileResult` es privado (sin break). `CopyEvent`/`Report` mantienen `String` (sin break en repo). Único break intencional: `#[non_exhaustive]` en `CopyError` — externos con `match` exhaustivo deberán añadir wildcard.
### P3 — Tests y robustez
- **P3.1** ⏳ Pendiente — `core/tests/`: plan (file/dir/exclude), overwrite `Never/IfNewer/IfDifferent`, verify `Size/Hash`, symlink Unix, cancel.
- **P3.2** ⏳ Pendiente — Integración end-to-end + fixtures con limpieza.

## 2. Hallazgos adicionales

- **H-1 (P1)** — `preserve_permissions` no preserva `mtime`. Rompe `IfDifferent/IfNewer` en la segunda pasada (siempre recopía). Afectado: `crates/core/src/engine.rs:380-384`. Estado: ⏳ Pendiente.
- **H-2 (P1)** — Sin `fsync`: `flush()` antes del rename no garantiza durabilidad. Falta `sync_all()` si se quiere semántica TeraCopy. Afectado: `crates/core/src/engine.rs:350-355`. Estado: ⏳ Pendiente.
- **H-3 (P0)** — Temporales `*.copyforge.tmp` huérfanos: el scanner no los excluye y no hay limpieza; un re-run los copiaría como ficheros. Afectado: `crates/core/src/engine.rs:407-411`, `crates/core/src/scanner.rs:118-122`. Estado: ⏳ Pendiente.
- **H-4 (P0)** — Sin validación `src==dst` ni `dst dentro de src`: riesgo de recursión explosiva/autosobrescritura. Afectado: `crates/core/src/scanner.rs:11-14`, `apps/cli/src/runner.rs`, `apps/gui/src/app.rs`. Estado: ⏳ Pendiente.
- **H-5 (P2)** — El progreso nunca llega a 100% con skips: `total_bytes` se calcula en el plan (`crates/core/src/engine.rs:34-36`) pero `Never/IfDifferent` luego saltan ficheros sin restar. Afectado además: `apps/cli/src/ui.rs`, `apps/gui/src/app.rs:303-307`. Estado: ⏳ Pendiente.
- **H-6 (P1)** — Symlink en Windows exige privilegio/Developer Mode. Falta fallback a “copiar contenido + warning” si da `EACCES`. Afectado: `crates/core/src/engine.rs:212-223`. Estado: ⏳ Pendiente.
- **H-7 (P0)** — `report.errors` sin cota: con millones de fallos puede agotar memoria. Capar (p.ej. 1000) + contador `errors_truncated`. Afectado: `crates/core/src/engine.rs:89-93`, `crates/core/src/events.rs`. Estado: ⏳ Pendiente.
