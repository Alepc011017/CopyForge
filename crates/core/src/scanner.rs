use std::collections::HashSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

use crate::{error::CopyError, options::CopyOptions, plan::*};
#[cfg(any(not(unix), test))]
use crate::platform;

/// Resuelve el destino efectivo **antes** de construir el plan.
///
/// Sin esto, `src=archivo → dst=directorio` generaba un `CopyTask` con
/// `dst` apuntando al directorio en vez de al fichero final.
///
/// | src | dst | resultado |
/// |---|---|---|
/// | archivo | directorio existente | `dst.join(file_name)` |
/// | archivo | no existe o es archivo | `dst` (renombrado / sobrescrito) |
/// | directorio | directorio existente | `dst.join(src_name)` |
/// | directorio | no existe | `dst` (se crea) |
///
/// Notas:
/// - `dst.is_dir()` sigue symlinks; si `dst` no existe devuelve `false`.
/// - `src` se inspecciona con `symlink_metadata` (sin seguir): un symlink
///   se trata como “tipo archivo” a efectos de resolución (un solo entry).
/// - Si `src` no tiene `file_name` (p. ej. `/`), se devuelve `dst` tal cual.
/// - Solo falla con `io::Error` si no se puede leer `src` (p. ej. no existe).
///   `build_plan` lo convierte a `CopyError::Io`.
pub fn build_effective_dest(src: &Path, dst: &Path) -> std::io::Result<PathBuf> {
    let src_meta = std::fs::symlink_metadata(src)?;
    let is_src_dir = src_meta.is_dir() && !src_meta.file_type().is_symlink();
    let dst_is_dir = dst.is_dir();

    if is_src_dir {
        if dst_is_dir {
            match src.file_name() {
                Some(name) => Ok(dst.join(name)),
                None => Ok(dst.to_path_buf()),
            }
        } else {
            Ok(dst.to_path_buf())
        }
    } else if dst_is_dir {
        match src.file_name() {
            Some(name) => Ok(dst.join(name)),
            None => Ok(dst.to_path_buf()),
        }
    } else {
        Ok(dst.to_path_buf())
    }
}

/// Valida que la pareja `(src, dst)` cruda no sea autodestructiva.
///
/// - `src == dst` → `InvalidDestination("src and dst are the same")`.
/// - `dst` dentro de `src` → `InvalidDestination("dst is inside src")`
///   (recursión explosiva: el destino crecería mientras se copia).
/// - `src` dentro de `dst` es **válido** (no error).
///
/// Diferencia con [`validate_effective_paths`]: esta función solo ve las
/// rutas tal como las pasa el usuario (defensa temprana, antes de tocar IO).
/// El caso `src=/a/b, dst=/a` pasa aquí pero resuelve a un destino efectivo
/// interior a `src`; ese hueco lo cierra `validate_effective_paths`, que se
/// llama dentro de [`build_plan`] tras [`build_effective_dest`].
///
/// Usa `canonicalize` cuando los paths existen (resuelve symlinks, `.`/`..`
/// y rutas largas de Windows). Si un path no existe, canonaliza el ancestro
/// existente más cercano y re-anexa el resto, así `dst` inexistente pero
/// interior a `src` también se detecta. Sin `unwrap`/`expect`.
pub fn validate_paths(src: &Path, dst: &Path) -> Result<(), CopyError> {
    let src_c = canonicalize_lossy(src);
    let dst_c = canonicalize_lossy(dst);
    if src_c == dst_c {
        return Err(CopyError::InvalidDestination {
            reason: "src and dst are the same".into(),
        });
    }
    if dst_c.starts_with(&src_c) {
        return Err(CopyError::InvalidDestination {
            reason: "dst is inside src".into(),
        });
    }
    Ok(())
}

/// Valida el destino **efectivo** tras [`build_effective_dest`].
///
/// Cierra el hueco que `validate_paths` no ve: `src=/a/b, dst=/a` pasa la
/// validación cruda (`/a` no está dentro de `/a/b`), pero el destino efectivo
/// es `/a/b` — igual a `src` — y sin este cheque el plan copiaría el árbol
/// sobre sí mismo en cada pasada (recursión exponencial).
///
/// - efectivo `== src` → `InvalidDestination("effective destination equals src")`.
/// - efectivo dentro de `src` → `InvalidDestination("effective destination is inside src")`.
///
/// Se llama dentro de [`build_plan`] justo después de resolver el destino
/// efectivo y antes de generar tareas. Sin `unwrap`/`expect`.
pub fn validate_effective_paths(src: &Path, dst: &Path) -> Result<(), CopyError> {
    let effective = build_effective_dest(src, dst).map_err(|e| CopyError::Io {
        path: src.to_path_buf(),
        source: e,
    })?;
    let src_c = canonicalize_lossy(src);
    let eff_c = canonicalize_lossy(&effective);
    if eff_c == src_c {
        return Err(CopyError::InvalidDestination {
            reason: "effective destination equals src".into(),
        });
    }
    if eff_c.starts_with(&src_c) {
        return Err(CopyError::InvalidDestination {
            reason: "effective destination is inside src".into(),
        });
    }
    Ok(())
}

fn canonicalize_lossy(p: &Path) -> PathBuf {
    let mut cur = p.to_path_buf();
    let mut missing: Vec<std::ffi::OsString> = Vec::new();
    loop {
        match cur.canonicalize() {
            Ok(c) => {
                let mut out = c;
                for comp in missing.iter().rev() {
                    out.push(comp);
                }
                return out;
            }
            Err(_) => {
                let Some(name) = cur.file_name().map(|s| s.to_os_string()) else {
                    return p.to_path_buf();
                };
                let Some(parent) = cur.parent().map(Path::to_path_buf) else {
                    return p.to_path_buf();
                };
                if parent == cur {
                    return p.to_path_buf();
                }
                missing.push(name);
                cur = parent;
            }
        }
    }
}

/// Suma los bytes de ficheros regulares bajo `target` (sin seguir symlinks).
///
/// P0.9: se usa para el cap `max_escape_bytes` del árbol externo cuando
/// `allow_symlink_escape=true`. Defensa en profundidad: evita copiar un
/// árbol externo gigante por accidente. Sin `unwrap`/`expect`.
fn external_tree_bytes(target: &Path) -> u64 {
    WalkDir::new(target)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

pub fn build_plan(src: &Path, dst: &Path, options: &CopyOptions) -> Result<CopyPlan, CopyError> {
    // Nota P1.4: no hay lista de exclusiones por defecto para temporales
    // (solo `options.exclusion` del usuario). Los `.copyforge-tmp-*`
    // huérfanos se limpian al inicio de `CopyEngine::run`, es decir, DESPUÉS
    // de construir el plan: un re-run puede planificar un huérfano como una
    // tarea que fallará de forma benigna (el fichero ya no existe). Para
    // excluirlos del plan, pasar `-x ".copyforge-tmp-*"` (raíz) o
    // `-x "**/.copyforge-tmp-*"` (cualquier profundidad).
    // P0.5: validar antes de cualquier IO (H-4): src==dst y dst⊂src.
    validate_paths(src, dst)?;

    // P0.8: symlink_metadata en vez de exists(), para aceptar un src que
    // sea un symlink roto (recreable con read_link). exists() sigue el
    // enlace y lo rechazaría con SourceNotFound.
    if std::fs::symlink_metadata(src).is_err() {
        return Err(CopyError::SourceNotFound(src.to_path_buf()));
    }

    // P0.2: resolver el destino final ANTES del walk, para que el plan
    // ya contenga la ruta correcta (file→dir, dir→dir).
    let effective_dst = build_effective_dest(src, dst).map_err(|e| CopyError::Io {
        path: src.to_path_buf(),
        source: e,
    })?;

    // P0.5-fix: el destino efectivo puede caer dentro de src aunque el
    // dst crudo no (p. ej. src=/a/b, dst=/a → efectivo /a/b). Validar
    // antes de generar tareas.
    validate_effective_paths(src, dst)?;

    // P0.8: raíz symlink rota: WalkDir falla con NotFound en profundidad 0
    // aunque el enlace sea recreable con read_link. Se emite una única
    // tarea Symlink. (Si no es symlink, se sigue al walk normal, que
    // reportará el error por entrada como antes.)
    if std::fs::metadata(src).is_err() {
        let link_meta = std::fs::symlink_metadata(src).map_err(|e| CopyError::Io {
            path: src.to_path_buf(),
            source: e,
        })?;
        if link_meta.file_type().is_symlink() {
            return Ok(CopyPlan {
                tasks: vec![CopyTask {
                    src: src.to_path_buf(),
                    dst: effective_dst,
                    size: link_meta.len(),
                    kind: TaskKind::Symlink,
                }],
                options: options.clone(),
                total_bytes: 0,
            });
        }
    }

    let excludes = build_globset(&options.exclusion)?;
    let mut tasks = Vec::new();
    let mut total_bytes = 0u64;

    // P0.9: base para detectar escapes (symlink seguido fuera de src).
    let src_c = canonicalize_lossy(src);
    let mut escape_bytes: u64 = 0;

    #[cfg(unix)]
    let mut visited_inodes: HashSet<(u64, u64)> = HashSet::new();
    #[cfg(not(unix))]
    let mut visited_dirs: HashSet<PathBuf> = HashSet::new();

    let walker = WalkDir::new(src)
        .follow_links(options.follow_symlinks)
        .into_iter()
        .filter_entry(|e| {
            if is_excluded(e.path(), src, &excludes) {
                return false;
            }
            if options.follow_symlinks {
                #[cfg(unix)]
                {
                    if let Ok(m) = std::fs::metadata(e.path()) {
                        if m.is_dir() {
                            let key = (m.dev(), m.ino());
                            if !visited_inodes.insert(key) {
                                return false;
                            }
                        }
                    }
                }
                #[cfg(not(unix))]
                {
                    if let Ok(m) = std::fs::metadata(e.path()) {
                        if m.is_dir() {
                            // P1.1a: canonicalización vía platform::.
                            match platform::canonicalize_long(e.path()) {
                                Ok(c) => {
                                    if !visited_dirs.insert(c) {
                                        return false;
                                    }
                                }
                                Err(_) => return false,
                            }
                        }
                    }
                }
            }
            true
        });

    for entry in walker {
        let entry = entry.map_err(|e| CopyError::Io {
            path: e.path().map(Path::to_path_buf).unwrap_or_default(),
            source: e.into(),
        })?;

        let path = entry.path();
        let rel = path.strip_prefix(src).unwrap_or(path);
        let target = if rel.as_os_str().is_empty() { effective_dst.clone() } else { effective_dst.join(rel) };

        // symlink_metadata (sin seguir) para clasificar bien: walkdir::DirEntry
        // en esta versión no expone symlink_metadata(), equivale a fs::symlink_metadata.
        let meta = std::fs::symlink_metadata(path).map_err(|e| CopyError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;

        let kind = if meta.file_type().is_symlink() {
            TaskKind::Symlink
        } else if meta.is_dir() {
            TaskKind::Dir
        } else {
            TaskKind::File
        };

        // P0.9: con follow_symlinks, un symlink cuyo target resuelto queda
        // fuera de src planificaría el árbol externo (p. ej. link → /).
        // Default seguro: abortar con SymlinkEscapesSource. Opt-in con
        // allow_symlink_escape (+ cap opcional max_escape_bytes).
        if kind == TaskKind::Symlink && options.follow_symlinks {
            let target_c = canonicalize_lossy(path);
            if !target_c.starts_with(&src_c) {
                if !options.allow_symlink_escape {
                    return Err(CopyError::SymlinkEscapesSource {
                        path: path.to_path_buf(),
                        target: target_c,
                    });
                }
                if let Some(limit) = options.max_escape_bytes {
                    let add = match std::fs::metadata(&target_c) {
                        Ok(m) if m.is_dir() => external_tree_bytes(&target_c),
                        Ok(m) => m.len(),
                        Err(_) => 0,
                    };
                    escape_bytes += add;
                    if escape_bytes > limit {
                        return Err(CopyError::EscapeCapExceeded {
                            limit,
                            current: escape_bytes,
                        });
                    }
                }
            }
        }

        if kind == TaskKind::File {
            total_bytes += meta.len();
        }

        tasks.push(CopyTask {
            src: path.to_path_buf(),
            dst: target,
            size: meta.len(),
            kind,
        });
    }

    Ok(CopyPlan { tasks, options: options.clone(), total_bytes })
}

fn build_globset(patterns: &[String]) -> Result<GlobSet, CopyError> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        // P1.5: estilo .gitignore — `*` NO cruza `/`, igual en ambos SO.
        // (La opción vive en GlobBuilder, no en GlobSetBuilder.)
        let g = GlobBuilder::new(p)
            .literal_separator(true)
            .build()
            .map_err(|e| CopyError::Io {
                path: PathBuf::from(p),
                source: std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()),
            })?;
        b.add(g);
    }
    b.build().map_err(|e| CopyError::Io {
        path: PathBuf::new(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()),
    })
}

/// Normaliza una ruta relativa a separador `/` para el matcheo de globs.
///
/// Usa `Path::components()` (sin manipulación de strings): cada componente
/// se une con `/`. En Windows los componentes ya vienen sin `\`, así que el
/// resultado es idéntico al de Unix. Sin `unwrap`/`expect`.
fn normalize_for_glob(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// ¿La ruta está excluida por los globs?
///
/// Matcheo agnóstico al SO: la ruta relativa se normaliza a `/` (ver
/// [`normalize_for_glob`]) y el `GlobSet` usa `literal_separator(true)`.
/// Los patrones SIEMPRE deben usar `/`, nunca `\`.
///
/// Antes, `target/**` no matcheaba `target/file.txt` igual en ambos SO
/// (en Windows las rutas llevan `\`); ahora sí, en Unix y Windows.
fn is_excluded(path: &Path, root: &Path, set: &GlobSet) -> bool {
    if set.is_empty() { return false; }
    let rel = path.strip_prefix(root).unwrap_or(path);
    set.is_match(normalize_for_glob(rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(p: &Path) {
        std::fs::write(p, b"hi").unwrap();
    }

    #[test]
    fn effective_file_into_existing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("a.txt");
        write_file(&src);
        let dst_dir = tmp.path().join("out");
        std::fs::create_dir(&dst_dir).unwrap();
        assert_eq!(
            build_effective_dest(&src, &dst_dir).unwrap(),
            dst_dir.join("a.txt")
        );
    }

    #[test]
    fn effective_file_to_new_path() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("a.txt");
        write_file(&src);
        let dst = tmp.path().join("renamed.txt");
        assert_eq!(build_effective_dest(&src, &dst).unwrap(), dst);
    }

    #[test]
    fn effective_dir_into_existing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("mydir");
        std::fs::create_dir(&src_dir).unwrap();
        let dst_dir = tmp.path().join("out");
        std::fs::create_dir(&dst_dir).unwrap();
        assert_eq!(
            build_effective_dest(&src_dir, &dst_dir).unwrap(),
            dst_dir.join("mydir")
        );
    }

    #[test]
    fn effective_dir_to_new_path() {
        let tmp = tempfile::tempdir().unwrap();
        let src_dir = tmp.path().join("mydir");
        std::fs::create_dir(&src_dir).unwrap();
        let dst = tmp.path().join("newdir");
        assert_eq!(build_effective_dest(&src_dir, &dst).unwrap(), dst);
    }

    #[test]
    fn validate_rejects_same_src_dst() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("a.txt");
        write_file(&src);
        let err = match validate_paths(&src, &src) {
            Ok(()) => panic!("expected InvalidDestination"),
            Err(e) => e,
        };
        match err {
            CopyError::InvalidDestination { reason } => assert!(reason.contains("same")),
            _ => panic!("expected InvalidDestination"),
        }
    }

    #[test]
    fn validate_rejects_dst_inside_src() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("mydir");
        std::fs::create_dir(&src).unwrap();
        // dst inexistente pero interior: ejercita canonicalize_lossy del padre.
        let dst = src.join("child");
        let err = match validate_paths(&src, &dst) {
            Ok(()) => panic!("expected InvalidDestination"),
            Err(e) => e,
        };
        match err {
            CopyError::InvalidDestination { reason } => assert!(reason.contains("inside")),
            _ => panic!("expected InvalidDestination"),
        }
    }

    #[test]
    fn validate_allows_src_inside_dst() {
        let tmp = tempfile::tempdir().unwrap();
        let dst = tmp.path().join("out");
        std::fs::create_dir(&dst).unwrap();
        let src = dst.join("inner");
        std::fs::create_dir(&src).unwrap();
        assert!(validate_paths(&src, &dst).is_ok());
    }

    #[test]
    fn validate_effective_rejects_dir_into_parent() {
        // src=/a/b, dst=/a: el dst crudo pasa validate_paths, pero el
        // efectivo (/a/b) equals src → InvalidDestination.
        let tmp = tempfile::tempdir().unwrap();
        let parent = tmp.path().join("a");
        std::fs::create_dir(&parent).unwrap();
        let src = parent.join("b");
        std::fs::create_dir(&src).unwrap();
        let err = match validate_effective_paths(&src, &parent) {
            Ok(()) => panic!("expected InvalidDestination"),
            Err(e) => e,
        };
        match err {
            CopyError::InvalidDestination { reason } => {
                assert!(reason.contains("effective destination"))
            }
            _ => panic!("expected InvalidDestination"),
        }
    }

    #[test]
    fn validate_effective_allows_disjoint_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("a").join("b");
        std::fs::create_dir_all(&src).unwrap();
        let dst = tmp.path().join("c");
        assert!(validate_effective_paths(&src, &dst).is_ok());
    }

    // Crear symlinks en Windows exige privilegios ausentes en runners
    // limpios: se compila en ambos SO pero solo se ejecuta en Unix.
    #[cfg_attr(windows, ignore)]
    #[test]
    fn build_plan_accepts_broken_symlink_src() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("broken");
        platform::symlink_file(Path::new("/nonexistent-target-xyz"), &src).unwrap();
        let dst = tmp.path().join("out-link");
        let plan = build_plan(&src, &dst, &CopyOptions::default()).unwrap();
        assert_eq!(plan.tasks.len(), 1);
        assert_eq!(plan.tasks[0].kind, TaskKind::Symlink);
        assert_eq!(plan.tasks[0].dst, dst);
    }

    fn escape_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("externo.txt"), b"12345678").unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir(&src).unwrap();
        let link = src.join("link-out");
        platform::symlink_dir(&outside, &link).unwrap();
        let dst = tmp.path().join("dst");
        (tmp, src, link, dst)
    }

    #[cfg_attr(windows, ignore)]
    #[test]
    fn escape_symlink_outside_is_rejected_by_default() {
        let (_tmp, src, link, dst) = escape_fixture();
        let opts = CopyOptions {
            follow_symlinks: true,
            ..CopyOptions::default()
        };
        let err = match build_plan(&src, &dst, &opts) {
            Ok(_) => panic!("expected SymlinkEscapesSource"),
            Err(e) => e,
        };
        match err {
            CopyError::SymlinkEscapesSource { path, target } => {
                assert_eq!(path, link);
                assert!(target.ends_with("outside"));
            }
            _ => panic!("expected SymlinkEscapesSource"),
        }
    }

    #[cfg_attr(windows, ignore)]
    #[test]
    fn escape_symlink_allowed_with_opt_in() {
        let (_tmp, src, _link, dst) = escape_fixture();
        let opts = CopyOptions {
            follow_symlinks: true,
            allow_symlink_escape: true,
            ..CopyOptions::default()
        };
        let plan = build_plan(&src, &dst, &opts).unwrap();
        assert!(plan.tasks.iter().any(|t| t.src.ends_with("externo.txt")));
    }

    #[cfg_attr(windows, ignore)]
    #[test]
    fn escape_cap_stops_big_external_tree() {
        let (_tmp, src, _link, dst) = escape_fixture();
        let opts = CopyOptions {
            follow_symlinks: true,
            allow_symlink_escape: true,
            max_escape_bytes: Some(1),
            ..CopyOptions::default()
        };
        let err = match build_plan(&src, &dst, &opts) {
            Ok(_) => panic!("expected EscapeCapExceeded"),
            Err(e) => e,
        };
        match err {
            CopyError::EscapeCapExceeded { limit, current } => {
                assert_eq!(limit, 1);
                assert_eq!(current, 8);
            }
            _ => panic!("expected EscapeCapExceeded"),
        }
    }

    #[test]
    fn glob_target_star_star_excludes_only_target() {
        let set = build_globset(&["target/**".to_string()]).unwrap();
        let root = Path::new("/root");
        assert!(is_excluded(&root.join("target/file.txt"), root, &set));
        assert!(is_excluded(&root.join("target/sub/deep.txt"), root, &set));
        assert!(!is_excluded(&root.join("src/file.txt"), root, &set));
        assert!(!is_excluded(&root.join("target2/file.txt"), root, &set));
    }

    #[test]
    fn normalize_for_glob_uses_forward_slashes() {
        // Sin '\\': en Unix es carácter literal del nombre, no separador.
        let p = PathBuf::from("dir/subdir/file.txt");
        assert_eq!(normalize_for_glob(&p), "dir/subdir/file.txt");
    }

    // Crear symlinks en Windows exige privilegios: se compila en ambos,
    // solo se ejecuta en Unix.
    #[cfg_attr(windows, ignore)]
    #[test]
    fn symlink_to_dir_classified_as_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = tmp.path().join("link");
        platform::symlink_dir(&real, &link).unwrap();
        let dst = tmp.path().join("dst");
        let opts = CopyOptions {
            follow_symlinks: false,
            ..CopyOptions::default()
        };
        let plan = build_plan(&link, &dst, &opts).unwrap();
        assert_eq!(plan.tasks.len(), 1);
        assert_eq!(plan.tasks[0].kind, TaskKind::Symlink);
    }
}