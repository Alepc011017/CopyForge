use std::collections::HashSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use globset::{Glob, GlobSet, GlobSetBuilder};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

use crate::{error::CopyError, options::CopyOptions, plan::*};

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

pub fn build_plan(src: &Path, dst: &Path, options: &CopyOptions) -> Result<CopyPlan, CopyError> {
    if !src.exists() {
        return Err(CopyError::SourceNotFound(src.to_path_buf()));
    }

    // P0.2: resolver el destino final ANTES del walk, para que el plan
    // ya contenga la ruta correcta (file→dir, dir→dir).
    let effective_dst = build_effective_dest(src, dst).map_err(|e| CopyError::Io {
        path: src.to_path_buf(),
        source: e,
    })?;

    let excludes = build_globset(&options.exclusion)?;
    let mut tasks = Vec::new();
    let mut total_bytes = 0u64;

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
                            match e.path().canonicalize() {
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
        let g = Glob::new(p).map_err(|e| CopyError::Io {
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

fn is_excluded(path: &Path, root: &Path, set: &GlobSet) -> bool {
    if set.is_empty() { return false; }
    let rel = path.strip_prefix(root).unwrap_or(path);
    set.is_match(rel)
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
}