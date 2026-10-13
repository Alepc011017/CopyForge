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
- **P1.1c** ✅ Hecho — `sync_dir` en `platform.rs` (Unix `open+sync_all`; Windows `CreateFileW` con `FILE_FLAG_BACKUP_SEMANTICS` + `FlushFileBuffers` + `CloseHandle` siempre; sin features nuevas: todo en `Win32_Storage_FileSystem` + `Win32_Foundation` ya declarados). Engine usa `platform::sync_dir`; eliminado `sync_parent_dir` + `warn_once`/`Arc<Once>` (ya sin usos). Doc de `Full` actualizado (funcional en ambas). Tests unix + windows. Sin commitear todavía.
- **P1.3** ⏳ Pendiente — `preserve_metadata` completo: `mtime` + `mode` en Unix; `readonly` + `mtime` en Windows. Incluye H-1 (mtime roto en `IfDifferent`/`IfNewer`). Afecta `engine.rs:380-384`.
- **P1.4** ⏳ Pendiente — Temporales robustos (H-3): `tempfile::NamedTempFile` en el mismo directorio + limpieza de huérfanos `*.copyforge.tmp` al inicio. Excluir el patrón en `scanner.rs:118-122`.
- **P1.5** ⏳ Pendiente — Globs cross-platform: `GlobBuilder::literal_separator(true)` + normalizar `/` antes de matchear. Afecta `scanner.rs:103-116`.
- **P1.6** ⏳ Pendiente — Fallback de symlink en Windows (H-6): si `symlink_file`/`symlink_dir` devuelve `EACCES`, copiar contenido + emitir `CopyEvent::SymlinkFallback` con warning. Afecta `engine.rs:212-223`.
  - Sub-caso: symlink roto apuntando a directorio en Windows → se recrea como file. Sin heurística fiable; documentar como limitación conocida y valorar fallback a copia del target o warning.
- **P1.7** ⏳ Pendiente — Validación insensible a mayúsculas (H-8): `validate_paths`/`validate_effective_paths` comparan byte a byte; en Windows/macOS `C:\Foo` y `c:\foo` son el mismo dir y bypassearían el chequeo. Normalizar caja al canonalizar en `platform.rs` (P1.1) cuando se cree.
- **P1.8** ⏳ Pendiente — Falso positivo menor (H-9): `src` symlink cuyo target es el `dst` (p. ej. `L→/real`, `dst=/real`) se rechaza como “same” aunque el efectivo (`/real/L`) sería válido. Workaround: pasar el `dst` explícito (`/real/L`). Mejora futura: no seguir el componente final symlink en `canonicalize_lossy` teniendo en cuenta `follow_symlinks`.

## P2 — Integración CLI/GUI

- **P2.1** ⏳ Pendiente — CLI: `tokio::signal::ctrl_c()` → `cancel_token().cancel()`; `verbose` → `EnvFilter` con `RUST_LOG`. Limpieza de `.tmp` al cancelar. Afecta `apps/cli/src/runner.rs:68-71`.
- **P2.2** ⏳ Pendiente — GUI: exponer `overwrite/verify/jobs/exclude`; botón Cancelar conectado al token; fix `/0`; etiqueta "bytes" en vez de "archivos"; `Handle` global en vez de `Runtime` por copia; `rfd::AsyncFileDialog`. Afecta `apps/gui/src/app.rs:58,69-74,97,117,303-307`.
- **P2.3** ⏳ Pendiente — Progreso coherente con skips (H-5): `total_bytes` debe decrementarse con `SkipReason::{Identical,Excluded,NewerExists}`; el progreso llega a 100%. Afecta `engine.rs:34-36`, `apps/cli/src/ui.rs`, `apps/gui/src/app.rs:303-307`.

## P3 — Tests y robustez

- **P3.1** ⏳ Pendiente — `crates/core/tests/`: `plan.rs` (4 casos), `overwrite.rs` (Never/IfNewer/IfDifferent/Always + SkipReason::Identical), `verify.rs` (Size/Hash), `symlink.rs` (cfg(unix), follow true/false), `cancel.rs` (cancela a mitad + limpieza .tmp), `src_eq_dst.rs` (P0.5).
- **P3.2** ⏳ Pendiente — E2E en `apps/cli/tests/`: copia real de árbol pequeño con `assert_cmd`, verificación de `--overwrite` y `--verify`, fixture con `tempfile::TempDir`, cleanup automático.

## Hallazgos resueltos (integrados arriba)
- H-1 → P1.3
- H-2 → P0.6a (DurabilityPolicy; Full en Windows → P1.1) + P0.6b (test integración)
- H-3 → P1.4
- H-4 → P0.5
- H-5 → P2.3
- H-6 → P1.6
- H-7 → P0.7
- H-8 → P1.7 (validación case-insensitive en Windows/macOS)
- H-9 → P1.8 (falso positivo symlink-src→target-dir, con workaround)
- H-10 → P0.9 (escape follow-symlink fuera de src: default seguro + opt-in + cap)

## Limitaciones conocidas (sin acción)
- TOCTOU validación→walk: el FS puede cambiar entre `validate_*` y el WalkDir/copia (p. ej. symlink sustituido tras validar). Sin locks no es cerrable del todo; la validación es defensa en profundidad, no garantía. No se actúa.
- `Path::starts_with` es por componentes (`/a/bb` NO está dentro de `/a/b`): verificado, sin falsos positivos por prefijo de string.
- Hardlinks src/dst hermanos: seguros (el rename del tmp rompe el link de `dst`, `src` intacto). Sin acción.

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
