# CopyForge — Plan consolidado

## 1. Plan priorizado original (P0 → P1 → P2 → P3)

### P0 — Correctitud del motor
- **P0.1** ✅ Hecho — Symlinks reales en `crates/core/src/engine.rs:136,141-228` (seguir vs recrear) + `FileResult::Symlinked` + `CopyEvent::Symlinked` + `Report{symlinks_created, symlinked}` en `crates/core/src/events.rs`. Scanner clasifica sin seguir con `std::fs::symlink_metadata` en `crates/core/src/scanner.rs:73-86` + detección de ciclos en `crates/core/src/scanner.rs:28-61`.
- **P0.2** 🔄 En curso — Caso `src=file → dst=dir`: si `src` es archivo y `dst` es directorio existente, el destino debe ser `dst.join(file_name)`. Hoy `crates/core/src/scanner.rs:71` usa `dst` tal cual. Afecta también a `apps/cli/src/runner.rs` y `apps/gui/src/app.rs` (validar antes de `build_plan`).
- **P0.3** ⏳ Pendiente — Concurrencia: mover `acquire_owned()` dentro del `spawn` en `crates/core/src/engine.rs:51-64`. Hoy serializa el arranque y limita el throughput paralelo.
- **P0.4** ⏳ Pendiente — Error tipado: `FileResult::Failed{String}` pierde `CopyError` (`crates/core/src/error.rs`, `crates/core/src/engine.rs:110-116`). Mapear a `CopyError::{Io,VerifyFailed,Cancelled}`.

### P1 — Capa multiplataforma (`platform.rs` nuevo)
- **P1.1** ⏳ Pendiente — `atomic_replace` Windows realmente atómico (`ReplaceFileW` / `MoveFileWithProgress`) en `crates/core/src/engine.rs:419-430`. Hoy es `remove+rename`, no atómico.
- **P1.2** ⏳ Pendiente — `preserve` completo: `mtime` + `readonly` Windows, no solo `mode` Unix, en `crates/core/src/engine.rs:380-384`.
- **P1.3** ⏳ Pendiente — Temporales robustos: `tempfile::NamedTempFile` en mismo dir + limpieza de huérfanos al inicio, en `crates/core/src/engine.rs:407-411`.
- **P1.4** ⏳ Pendiente — Globs cross-platform: `GlobBuilder::literal_separator(true)` + normalizar `/` en `crates/core/src/scanner.rs:103-116`.

### P2 — Integración CLI/GUI
- **P2.1** ⏳ Pendiente — CLI: `Ctrl-C` → `cancel_token().cancel()` en `apps/cli/src/runner.rs`; `verbose` → `EnvFilter`.
- **P2.2** ⏳ Pendiente — GUI: exponer `overwrite/verify/jobs/exclude`, botón Cancelar, fix `/0` (`apps/gui/src/app.rs:97,117`), etiqueta bytes vs archivos (`apps/gui/src/app.rs:303-307`), reutilizar `Handle` en vez de `Runtime` por copia (`apps/gui/src/app.rs:69-74`).
- **P2.3** ⏳ Pendiente — Progreso coherente con skips (ver H-5).

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
