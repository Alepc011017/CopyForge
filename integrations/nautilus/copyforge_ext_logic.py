"""Lógica pura de la extensión de Nautilus de CopyForge.

Este módulo **no importa `gi`** para poder testearlo sin nautilus-python ni
GTK. La extensión (`copyforge_extension.py`) importa de aquí:

- `parse_clipboard`: lee `x-special/gnome-copied-files` / `text/uri-list`.
- `filter_sources`: descarta orígenes que coinciden con el destino o lo
  contienen (evita el `rm -rf` autodestructivo del modo mover).
- `gui_bin`: localiza el binario de la GUI (`COPYFORGE_GUI_BIN` o `PATH`).
- `clipboard_read_commands` / `clipboard_write_commands` / `clipboard_payload`:
  portapapeles vía `wl-paste`/`wl-copy` (Wayland) o `xclip` (X11).

El orden de lectura es deliberado: `x-special/gnome-copied-files` **primero**
porque es el único tipo que distingue copiar de cortar; `text/uri-list` (y
`Gdk.FileList`) no llevan esa información.
"""

import os
import shutil
from urllib.parse import quote, unquote, urlparse

GNOME_COPIED_FILES = "x-special/gnome-copied-files"
URI_LIST = "text/uri-list"

# Orden de búsqueda del binario: primero el de la GUI en Tauri.
GUI_BIN_NAMES = ("copyforge-gui-tauri", "copyforge", "CopyForge")


def parse_clipboard(data):
    """Devuelve `(accion, rutas)` a partir del portapapeles.

    `accion` es `"copy"` o `"cut"` (la primera línea de
    `x-special/gnome-copied-files`); si el formato no la trae se asume `"copy"`.
    Ignora líneas vacías, comentarios (`#`) y cualquier cosa que no sea una URL
    `file://`.
    """
    lines = data.splitlines()
    action = "copy"
    if lines and lines[0].strip() in ("copy", "cut"):
        action = lines[0].strip()
        lines = lines[1:]

    paths = []
    for line in lines:
        line = line.strip()
        if not line or line.startswith("#") or not line.startswith("file://"):
            continue
        path = unquote(urlparse(line).path)
        if path:
            paths.append(path)
    return action, paths


def filter_sources(sources, destination):
    """Separa orígenes válidos de los que hay que descartar.

    Devuelve `(validos, descartados)` donde `descartados` es una lista de
    `(ruta, motivo)`. Se comparan rutas absolutas **por componentes**
    (`os.sep`), no por prefijo de cadena.
    """
    dst_abs = os.path.abspath(destination)
    valid, skipped = [], []
    for src in sources:
        if not (os.path.exists(src) or os.path.islink(src)):
            skipped.append((src, "no existe"))
            continue
        src_abs = os.path.abspath(src)
        if src_abs == dst_abs:
            skipped.append((src, "es el destino"))
        elif src_abs.startswith(dst_abs + os.sep):
            skipped.append((src, "está dentro del destino"))
        elif dst_abs.startswith(src_abs + os.sep):
            skipped.append((src, "contiene al destino"))
        else:
            valid.append(src)
    return valid, skipped


def gui_bin(env=None, which=None):
    """Ruta al binario de la GUI, o `None`.

    Orden: `$COPYFORGE_GUI_BIN` (si no está vacío) → `copyforge-gui-tauri` →
    `copyforge` → `CopyForge` en `PATH`.
    """
    env = os.environ if env is None else env
    which = shutil.which if which is None else which

    configured = (env.get("COPYFORGE_GUI_BIN") or "").strip()
    if configured:
        return configured
    for name in GUI_BIN_NAMES:
        found = which(name)
        if found:
            return found
    return None


def clipboard_read_commands(which=None):
    """Comandos para leer el portapapeles, con `gnome-copied-files` primero."""
    which = shutil.which if which is None else which
    cmds = []
    if which("wl-paste"):
        cmds.append(["wl-paste", "--no-newline", "--type", GNOME_COPIED_FILES])
        cmds.append(["wl-paste", "--no-newline", "--type", URI_LIST])
    if which("xclip"):
        cmds.append(["xclip", "-selection", "clipboard", "-t", GNOME_COPIED_FILES, "-o"])
        cmds.append(["xclip", "-selection", "clipboard", "-t", URI_LIST, "-o"])
    return cmds


def clipboard_write_commands(which=None):
    """Comandos para escribir el portapapeles con el tipo que distingue cut."""
    which = shutil.which if which is None else which
    cmds = []
    if which("wl-copy"):
        cmds.append(["wl-copy", "--type", GNOME_COPIED_FILES])
    if which("xclip"):
        cmds.append(["xclip", "-selection", "clipboard", "-t", GNOME_COPIED_FILES, "-i"])
    return cmds


def clipboard_payload(action, paths):
    """Contenido de `x-special/gnome-copied-files` para `action` y `paths`."""
    return action + "\n" + "\n".join("file://" + quote(p) for p in paths) + "\n"
