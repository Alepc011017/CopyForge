"""CopyForge — extensión de Nautilus (nautilus-python, GTK4).

Menús: fondo → "Pegar con CopyForge" (copy) / "Mover aquí con CopyForge"
(move); selección → "Copiar con CopyForge" (copy) / "Mover con CopyForge"
(cut), que escriben el portapapeles.

La extensión no copia: lanza `copyforge-gui-tauri copy|move <src...> --to <dst>`
y la GUI encola la tarea y la ejecuta con el motor de `crates/core`.
"""

import os
import shutil
import subprocess
import sys
import threading

import gi

try:
    gi.require_version("Nautilus", "4.1")
except ValueError:
    gi.require_version("Nautilus", "4.0")
gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")

from gi.repository import Gdk, GLib, GObject, Nautilus

# `nautilus-python` no siempre añade el directorio de extensiones a sys.path.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from copyforge_ext_logic import (  # noqa: E402
    clipboard_payload, clipboard_read_commands, clipboard_write_commands,
    filter_sources, gui_bin, parse_clipboard,
)

LOG_FILE = os.environ.get(
    "COPYFORGE_LOG",
    os.path.join(
        os.environ.get("XDG_STATE_HOME", os.path.expanduser("~/.local/state")),
        "copyforge", "nautilus.log",
    ),
)
MISSING_BIN = (
    "No se encontró el binario de CopyForge.\n\nDefine COPYFORGE_GUI_BIN con "
    "la ruta al ejecutable (p. ej. target/release/copyforge-gui-tauri) o "
    "ponlo en PATH."
)


def log(msg):
    print(f"CopyForge: {msg}", file=sys.stderr)
    try:
        os.makedirs(os.path.dirname(LOG_FILE), exist_ok=True)
        with open(LOG_FILE, "a", encoding="utf-8", errors="replace") as f:
            f.write(f"{msg}\n")
    except Exception:
        pass


def _notify_error(msg):
    log(f"ERROR: {msg}")
    if not shutil.which("zenity"):
        return  # el mensaje ya está en stderr y en el log
    try:
        subprocess.Popen(
            ["zenity", "--error", "--title=CopyForge", f"--text={msg}"],
            start_new_session=True,
        )
    except Exception as e:
        log(f"zenity falló: {e}")


def _gui_bin():
    """Binario de la GUI si existe y es ejecutable; si no, `None`."""
    binary = gui_bin()
    if binary and os.path.isfile(binary) and os.access(binary, os.X_OK):
        return binary
    return None


class CopyForgeExtension(GObject.GObject, Nautilus.MenuProvider):
    def __init__(self):
        super().__init__()
        tools = [cmd[0] for cmd in clipboard_read_commands()]
        log(f"init: gui={gui_bin()} portapapeles={tools or 'ninguno'}")

    # ---------- lectura del portapapeles --------------------------------

    def _read_clipboard_chain(self, on_ok, on_err):
        """`gnome-copied-files` primero (detecta `cut`); Gdk como fallback."""

        def try_gdk(err):
            log(f"CLI falló ({err}); probando Gdk")
            self._read_files(on_ok, lambda e: self._read_uri(on_ok, on_err))

        self._read_cli(on_ok, try_gdk)

    def _read_cli(self, on_ok, on_err):
        def worker():
            errors = []
            for cmd in clipboard_read_commands():
                try:
                    r = subprocess.run(cmd, capture_output=True, text=True, timeout=5)
                except Exception as e:
                    errors.append(f"{cmd[0]}: {e}")
                    continue
                if r.returncode != 0 or not r.stdout.strip():
                    continue
                action, paths = parse_clipboard(r.stdout)
                if paths:
                    GLib.idle_add(on_ok, action, paths)
                    return
            GLib.idle_add(on_err, "; ".join(errors) or "sin wl-paste/xclip")

        threading.Thread(target=worker, daemon=True).start()

    def _read_files(self, on_ok, on_err):
        def done(clip, result, _data):
            try:
                value = clip.read_value_finish(result)
                flist = value if hasattr(value, "get_files") else value.get_value()
                paths = [f.get_path() for f in flist.get_files() if f.get_path()]
            except Exception as e:
                on_err(f"FileList: {e}")
                return
            # Gdk.FileList no distingue copiar de cortar: se asume "copy".
            if paths:
                on_ok("copy", paths)
            else:
                on_err("FileList sin rutas")

        try:
            Gdk.Display.get_default().get_clipboard().read_value_async(
                Gdk.FileList, GLib.PRIORITY_DEFAULT, None, done, None
            )
        except Exception as e:
            on_err(f"FileList: {e}")

    def _read_uri(self, on_ok, on_err):
        def done(clip, result, _data):
            try:
                ret = clip.read_finish(result)
                stream = ret if hasattr(ret, "read_bytes") else ret.stream
                text = stream.read_bytes(65536, None).get_data().decode(
                    "utf-8", "replace"
                )
                action, paths = parse_clipboard(text)
            except Exception as e:
                on_err(f"uri-list: {e}")
                return
            if paths:
                on_ok(action, paths)
            else:
                on_err("uri-list sin rutas")

        try:
            Gdk.Display.get_default().get_clipboard().read_async(
                ["text/uri-list"], GLib.PRIORITY_DEFAULT, None, done, None
            )
        except Exception as e:
            on_err(f"uri-list: {e}")

    # ---------- escritura del portapapeles ------------------------------

    def _write_clipboard(self, action, paths):
        paths = [p for p in paths if p]
        if not paths:
            _notify_error("No hay rutas válidas.")
            return
        payload = clipboard_payload(action, paths).encode("utf-8")
        for cmd in clipboard_write_commands():
            try:
                proc = subprocess.Popen(cmd, stdin=subprocess.PIPE)
                proc.communicate(payload, timeout=2)
                if proc.returncode == 0:
                    log(f"portapapeles: {action} {len(paths)} ruta(s) vía {cmd[0]}")
                    return
            except Exception:
                continue
        _notify_error(
            "No se pudo escribir en el portapapeles: "
            "instala wl-clipboard (Wayland) o xclip (X11)."
        )

    # ---------- lanzamiento ---------------------------------------------

    def _launch(self, mode, destination, sources):
        binary = _gui_bin()
        if binary is None:
            _notify_error(MISSING_BIN)
            return
        valid, skipped = filter_sources(sources, destination)
        for src, why in skipped:
            log(f"origen descartado: {src} ({why})")
        if not valid:
            _notify_error("Ningún origen válido: coinciden con el destino o lo contienen.")
            return
        try:
            subprocess.Popen(
                [binary, mode, *valid, "--to", destination,
                 "--overwrite", "always", "--verify", "size"],
                start_new_session=True,
            )
            log(f"lanzado: {mode} {len(valid)} origen(es) -> {destination}")
        except Exception as e:
            _notify_error(f"No se pudo lanzar CopyForge: {e}")

    # ---------- menús ---------------------------------------------------

    @staticmethod
    def _item(name, label, tip, handler, arg):
        item = Nautilus.MenuItem(name=name, label=label, tip=tip)
        item.connect("activate", handler, arg)
        return item

    def get_background_items(self, *args):
        folder = args[-1] if args else None
        if folder is None or not folder.is_directory() or folder.get_uri_scheme() != "file":
            return []
        return [
            self._item("CopyForge::Paste", "Pegar con CopyForge",
                       "Copiar aquí los archivos del portapapeles",
                       self._on_paste, folder),
            self._item("CopyForge::MoveHere", "Mover aquí con CopyForge",
                       "Mover aquí los archivos del portapapeles",
                       self._on_move_here, folder),
        ]

    def get_file_items(self, *args):
        files = args[-1] if args else None
        if not files or any(f.get_uri_scheme() != "file" for f in files):
            return []
        return [
            self._item("CopyForge::Copy", "Copiar con CopyForge",
                       "Marcar para copiar con CopyForge", self._on_copy, files),
            self._item("CopyForge::Cut", "Mover con CopyForge",
                       "Marcar para mover con CopyForge", self._on_cut, files),
        ]

    # ---------- callbacks -----------------------------------------------

    def _destination(self, folder):
        try:
            destination = folder.get_location().get_path()
        except Exception as e:
            _notify_error(f"No se pudo obtener la ruta destino: {e}")
            return None
        if not destination:
            _notify_error("Ruta destino vacía.")
        return destination

    def _on_paste(self, _menu, folder):
        destination = self._destination(folder)
        if destination:
            self._read_clipboard_chain(
                lambda action, srcs: self._launch(
                    "move" if action == "cut" else "copy", destination, srcs
                ),
                lambda err: _notify_error(f"No se pudo leer el portapapeles: {err}"),
            )

    def _on_move_here(self, _menu, folder):
        destination = self._destination(folder)
        if destination:
            self._read_clipboard_chain(
                lambda _action, srcs: self._launch("move", destination, srcs),
                lambda err: _notify_error(f"No se pudo leer el portapapeles: {err}"),
            )

    def _on_copy(self, _menu, files):
        self._write_clipboard("copy", [f.get_location().get_path() for f in files])

    def _on_cut(self, _menu, files):
        self._write_clipboard("cut", [f.get_location().get_path() for f in files])
