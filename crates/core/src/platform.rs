//! Capa multiplataforma (P1.1a).
//!
//! Todo el `#[cfg]` de estas operaciones vive en este módulo; el resto del
//! crate llama a `platform::*` sin `cfg`. Funciones libres, sin traits.
//! Sin `unwrap`/`expect` en productivo.

use std::io;
use std::path::{Path, PathBuf};

/// Crea un enlace simbólico a un fichero.
///
/// `src` = destino existente al que apuntará el enlace;
/// `dst` = ruta del enlace a crear.
#[cfg(unix)]
pub fn symlink_file(src: &Path, dst: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(src, dst)
}

/// Crea un enlace simbólico a un fichero.
///
/// `src` = destino existente al que apuntará el enlace;
/// `dst` = ruta del enlace a crear.
#[cfg(windows)]
pub fn symlink_file(src: &Path, dst: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_file(src, dst)
}

/// Crea un enlace simbólico a un directorio.
///
/// `src` = destino existente al que apuntará el enlace;
/// `dst` = ruta del enlace a crear.
#[cfg(unix)]
pub fn symlink_dir(src: &Path, dst: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(src, dst)
}

/// Crea un enlace simbólico a un directorio.
///
/// `src` = destino existente al que apuntará el enlace;
/// `dst` = ruta del enlace a crear.
#[cfg(windows)]
pub fn symlink_dir(src: &Path, dst: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(src, dst)
}

/// Fija (`true`) o quita (`false`) el solo-lectura.
///
/// Unix: quita los bits de escritura (`mode & !0o222`); al reactivar,
/// restaura el bit de escritura del propietario (`mode | 0o200`).
/// Windows: `permissions().set_readonly()`.
#[cfg(unix)]
pub fn set_readonly(path: &Path, readonly: bool) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    let mut mode = perms.mode();
    if readonly {
        mode &= !0o222;
    } else {
        mode |= 0o200;
    }
    perms.set_mode(mode);
    std::fs::set_permissions(path, perms)
}

/// Fija (`true`) o quita (`false`) el solo-lectura.
///
/// Unix: quita los bits de escritura (`mode & !0o222`); al reactivar,
/// restaura el bit de escritura del propietario (`mode | 0o200`).
/// Windows: `permissions().set_readonly()`.
#[cfg(windows)]
pub fn set_readonly(path: &Path, readonly: bool) -> io::Result<()> {
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_readonly(readonly);
    std::fs::set_permissions(path, perms)
}

/// ¿El path está oculto?
///
/// Unix: el nombre empieza por `.` (sin `file_name` → `false`).
/// Windows: atributo `FILE_ATTRIBUTE_HIDDEN`.
#[cfg(unix)]
pub fn is_hidden(path: &Path) -> io::Result<bool> {
    Ok(path
        .file_name()
        .map(|n| n.to_string_lossy().starts_with('.'))
        .unwrap_or(false))
}

/// ¿El path está oculto?
///
/// Unix: el nombre empieza por `.` (sin `file_name` → `false`).
/// Windows: atributo `FILE_ATTRIBUTE_HIDDEN`.
#[cfg(windows)]
pub fn is_hidden(path: &Path) -> io::Result<bool> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{
        GetFileAttributesW, FILE_ATTRIBUTE_HIDDEN, INVALID_FILE_ATTRIBUTES,
    };
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` es NUL-terminado y vive hasta el fin de la llamada.
    let attrs = unsafe { GetFileAttributesW(windows::core::PCWSTR(wide.as_ptr())) };
    if attrs == INVALID_FILE_ATTRIBUTES {
        return Err(io::Error::last_os_error());
    }
    Ok((attrs & FILE_ATTRIBUTE_HIDDEN.0) != 0)
}

/// Canonicaliza y devuelve forma larga (para comparar y evadir `MAX_PATH`).
///
/// Unix: `fs::canonicalize`. Windows: `fs::canonicalize` + prefijo `\\?\`
/// (idempotente: si ya lo trae, se devuelve tal cual; `\\servidor\...` se
/// convierte a `\\?\UNC\servidor\...`).
#[cfg(unix)]
pub fn canonicalize_long(path: &Path) -> io::Result<PathBuf> {
    std::fs::canonicalize(path)
}

/// Reemplaza `dst` por `src` de forma atómica.
///
/// Unix: `rename` sobrescribe atómicamente.
///
/// Windows: `rename`/`MoveFile` falla si `dst` existe, y el apaño
/// `remove+rename` NO basta: son dos llamadas — si el proceso muere entre
/// ambas no existe ni origen ni destino, y los lectores concurrentes pueden
/// observar el fichero ausente. `ReplaceFileW` conmuta de forma atómica;
/// como falla con `ERROR_FILE_NOT_FOUND` si `dst` no existe, en ese caso se
/// recurre a `MoveFileExW` con `MOVEFILE_REPLACE_EXISTING`. Cualquier otro
/// error se devuelve como `io::Error` conservando su código.
#[cfg(unix)]
pub fn atomic_replace(src: &Path, dst: &Path) -> io::Result<()> {
    std::fs::rename(src, dst)
}

/// Reemplaza `dst` por `src` de forma atómica.
///
/// Unix: `rename` sobrescribe atómicamente.
///
/// Windows: `rename`/`MoveFile` falla si `dst` existe, y el apaño
/// `remove+rename` NO basta: son dos llamadas — si el proceso muere entre
/// ambas no existe ni origen ni destino, y los lectores concurrentes pueden
/// observar el fichero ausente. `ReplaceFileW` conmuta de forma atómica;
/// como falla con `ERROR_FILE_NOT_FOUND` si `dst` no existe, en ese caso se
/// recurre a `MoveFileExW` con `MOVEFILE_REPLACE_EXISTING`. Cualquier otro
/// error se devuelve como `io::Error` conservando su código.
#[cfg(windows)]
pub fn atomic_replace(src: &Path, dst: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, ReplaceFileW, MOVEFILE_REPLACE_EXISTING, REPLACE_FILE_FLAGS,
    };
    use windows::core::{HRESULT, PCWSTR};

    fn wide_null(p: &Path) -> Vec<u16> {
        p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    }

    // Nótese el orden: reemplazado=dst, reemplazo=src (el temporal).
    // SAFETY: los buffers son NUL-terminados y viven hasta el fin de la llamada.
    let dst_w = wide_null(dst);
    let src_w = wide_null(src);
    let replaced = unsafe {
        ReplaceFileW(
            PCWSTR(dst_w.as_ptr()),
            PCWSTR(src_w.as_ptr()),
            PCWSTR::null(),
            REPLACE_FILE_FLAGS::default(),
            None,
            None,
        )
    };
    match replaced {
        Ok(()) => Ok(()),
        Err(e) if e.code() == HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0) => {
            let moved = unsafe {
                MoveFileExW(
                    PCWSTR(src_w.as_ptr()),
                    PCWSTR(dst_w.as_ptr()),
                    MOVEFILE_REPLACE_EXISTING,
                )
            };
            moved.map_err(io::Error::from)
        }
        Err(e) => Err(io::Error::from(e)),
    }
}

/// Canonicaliza y devuelve forma larga (para comparar y evadir `MAX_PATH`).
///
/// Unix: `fs::canonicalize`. Windows: `fs::canonicalize` + prefijo `\\?\`
/// (idempotente: si ya lo trae, se devuelve tal cual; `\\servidor\...` se
/// convierte a `\\?\UNC\servidor\...`).
#[cfg(windows)]
pub fn canonicalize_long(path: &Path) -> io::Result<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let canon = std::fs::canonicalize(path)?;
    let w: Vec<u16> = canon.as_os_str().encode_wide().collect();
    const VERBATIM: &[u16] = &['\\' as u16, '\\' as u16, '?' as u16, '\\' as u16];
    const UNC: &[u16] = &['\\' as u16, '\\' as u16];
    const DEVICE_UNC: &[u16] = &[
        '\\' as u16, '\\' as u16, '?' as u16, '\\' as u16, 'U' as u16, 'N' as u16,
        'C' as u16, '\\' as u16,
    ];
    if w.starts_with(VERBATIM) {
        return Ok(canon);
    }
    let mut out: Vec<u16> = Vec::with_capacity(w.len() + 8);
    if w.starts_with(UNC) {
        out.extend_from_slice(DEVICE_UNC);
        out.extend_from_slice(&w[2..]);
    } else {
        out.extend_from_slice(VERBATIM);
        out.extend_from_slice(&w);
    }
    Ok(PathBuf::from(OsString::from_wide(&out)))
}

/// Sincroniza un directorio a disco (hace durable las entradas creadas
/// por renames recientes dentro de él).
///
/// Unix: `File::open(dir)` + `sync_all()`.
///
/// Windows: `CreateFileW` con `FILE_FLAG_BACKUP_SEMANTICS` (sin ese flag
/// no se pueden abrir directorios) + `FlushFileBuffers`, cerrando el
/// handle siempre (`CloseHandle`).
/// verificado por CI windows-latest, ver .github/workflows/ci.yml
///
/// Sin `unwrap`/`expect`.
#[cfg(unix)]
pub fn sync_dir(dir: &Path) -> io::Result<()> {
    let f = std::fs::File::open(dir)?;
    f.sync_all()
}

/// Sincroniza un directorio a disco (hace durable las entradas creadas
/// por renames recientes dentro de él).
///
/// Unix: `File::open(dir)` + `sync_all()`.
///
/// Windows: `CreateFileW` con `FILE_FLAG_BACKUP_SEMANTICS` (sin ese flag
/// no se pueden abrir directorios) + `FlushFileBuffers`, cerrando el
/// handle siempre (`CloseHandle`).
/// verificado por CI windows-latest, ver .github/workflows/ci.yml
///
/// Sin `unwrap`/`expect`.
#[cfg(windows)]
pub fn sync_dir(dir: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::{CloseHandle, GENERIC_READ};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FlushFileBuffers, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::core::PCWSTR;

    fn wide_null(p: &Path) -> Vec<u16> {
        p.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    }

    // SAFETY: el buffer es NUL-terminado y vive hasta el fin de la llamada.
    // El handle se cierra siempre antes de retornar.
    let dir_w = wide_null(dir);
    let handle = unsafe {
        CreateFileW(
            PCWSTR(dir_w.as_ptr()),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            None,
        )
    }
    .map_err(io::Error::from)?;
    let flushed = unsafe { FlushFileBuffers(handle) }.map_err(io::Error::from);
    let _ = unsafe { CloseHandle(handle) };
    flushed
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn write_file(p: &Path) {
        std::fs::write(p, b"platform").unwrap();
    }

    #[test]
    fn symlink_file_creates_link() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target.txt");
        write_file(&target);
        let link = tmp.path().join("link.txt");
        symlink_file(&target, &link).unwrap();
        let meta = std::fs::symlink_metadata(&link).unwrap();
        assert!(meta.file_type().is_symlink());
        assert_eq!(std::fs::read_link(&link).unwrap(), target);
    }

    #[test]
    fn set_readonly_clears_write_bit() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("rw.txt");
        write_file(&f);
        set_readonly(&f, true).unwrap();
        assert!(std::fs::metadata(&f).unwrap().permissions().readonly());
        set_readonly(&f, false).unwrap();
        assert!(!std::fs::metadata(&f).unwrap().permissions().readonly());
    }

    #[test]
    fn is_hidden_detects_dotfiles() {
        let tmp = tempfile::tempdir().unwrap();
        let hidden = tmp.path().join(".oculto");
        write_file(&hidden);
        assert!(is_hidden(&hidden).unwrap());
        let plain = tmp.path().join("visible.txt");
        write_file(&plain);
        assert!(!is_hidden(&plain).unwrap());
    }

    #[test]
    fn canonicalize_long_returns_absolute() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("a.txt");
        write_file(&f);
        let canon = canonicalize_long(&f).unwrap();
        assert!(canon.is_absolute());
        assert_eq!(canon.file_name(), f.file_name());
    }

    #[test]
    fn atomic_replace_overwrites() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.txt");
        let b = tmp.path().join("b.txt");
        std::fs::write(&a, b"nuevo").unwrap();
        std::fs::write(&b, b"viejo").unwrap();
        atomic_replace(&a, &b).unwrap();
        assert_eq!(std::fs::read(&b).unwrap(), b"nuevo");
        assert!(!a.exists());
    }

    #[test]
    fn sync_dir_flushes_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("a.txt");
        std::fs::write(&f, b"sync").unwrap();
        sync_dir(tmp.path()).unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"sync");
    }
}

#[cfg(all(test, windows))]
mod tests_windows {
    use super::*;

    #[test]
    fn atomic_replace_overwrites() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.txt");
        let b = tmp.path().join("b.txt");
        std::fs::write(&a, b"nuevo").unwrap();
        std::fs::write(&b, b"viejo").unwrap();
        atomic_replace(&a, &b).unwrap();
        assert_eq!(std::fs::read(&b).unwrap(), b"nuevo");
        assert!(!a.exists());
    }

    #[test]
    fn sync_dir_flushes_parent() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("a.txt");
        std::fs::write(&f, b"sync").unwrap();
        sync_dir(tmp.path()).unwrap();
        assert_eq!(std::fs::read(&f).unwrap(), b"sync");
    }
}
