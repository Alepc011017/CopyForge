use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use globset::{Glob, GlobSet, GlobSetBuilder};

use crate::{error::CopyError, options::CopyOptions, plan::*};

pub fn build_plan(src: &Path, dst: &Path, options: &CopyOptions) -> Result<CopyPlan, CopyError> {
    if !src.exists() {
        return Err(CopyError::SourceNotFound(src.to_path_buf()));
    }

    let excludes = build_globset(&options.exclusion)?;
    let mut tasks = Vec::new();
    let mut total_bytes = 0u64;

    let walker = WalkDir::new(src)
        .follow_links(options.follow_symlinks)
        .into_iter()
        .filter_entry(|e| !is_excluded(e.path(), src, &excludes));

    for entry in walker {
        let entry = entry.map_err(|e| CopyError::Io {
            path: e.path().map(Path::to_path_buf).unwrap_or_default(),
            source: e.into(),
        })?;

        let path = entry.path();
        let rel = path.strip_prefix(src).unwrap_or(path);
        let target = if rel.as_os_str().is_empty() { dst.to_path_buf() } else { dst.join(rel) };

        let meta = entry.metadata().map_err(|e| CopyError::Io {
            path: path.to_path_buf(),
            source: e.into(),
        })?;

        let kind = if meta.is_dir() {
            TaskKind::Dir
        } else if meta.file_type().is_symlink() {
            TaskKind::Symlink
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