#!/usr/bin/env bash
# Instala (o desinstala) la extensión de Nautilus de CopyForge.
set -euo pipefail

SRC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEST_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/nautilus-python/extensions"
FILES=(copyforge_extension.py copyforge_ext_logic.py)

usage() {
    cat <<'EOF'
Uso: ./install.sh [--uninstall]

  (sin argumentos)  Instala la extensión en
                    ~/.local/share/nautilus-python/extensions
  --uninstall       Elimina los ficheros instalados (y su __pycache__)
  -h, --help        Muestra esta ayuda
EOF
}

hint_nautilus_python() {
    cat <<'EOF'
Falta nautilus-python (o su typelib de Nautilus). Instálalo según tu distro:

  Debian / Ubuntu   sudo apt install python3-nautilus
  Fedora            sudo dnf install nautilus-python
  Arch              sudo pacman -S python-nautilus
  openSUSE          sudo zypper install python3-nautilus

Después vuelve a ejecutar ./install.sh.
EOF
}

has_nautilus_python() {
    python3 - <<'PY' >/dev/null 2>&1
import gi
try:
    gi.require_version("Nautilus", "4.1")
except ValueError:
    gi.require_version("Nautilus", "4.0")
PY
}

case "${1:-}" in
    "")
        ;;
    --uninstall)
        for f in "${FILES[@]}"; do
            if [ -e "$DEST_DIR/$f" ]; then
                rm -f "$DEST_DIR/$f"
                echo "eliminado: $DEST_DIR/$f"
            fi
        done
        rm -rf "$DEST_DIR/__pycache__"
        echo
        echo "Reinicia Nautilus para descargar la extensión:  nautilus -q"
        exit 0
        ;;
    -h|--help)
        usage
        exit 0
        ;;
    *)
        usage >&2
        exit 2
        ;;
esac

command -v python3 >/dev/null 2>&1 || {
    echo "Error: falta python3." >&2
    exit 1
}
if ! has_nautilus_python; then
    hint_nautilus_python >&2
    exit 1
fi

for f in "${FILES[@]}"; do
    [ -f "$SRC_DIR/$f" ] || {
        echo "Error: falta $SRC_DIR/$f" >&2
        exit 1
    }
done

mkdir -p "$DEST_DIR"
for f in "${FILES[@]}"; do
    install -m 0644 "$SRC_DIR/$f" "$DEST_DIR/$f"
    echo "instalado: $DEST_DIR/$f"
done

echo
if ! command -v wl-paste >/dev/null 2>&1 && ! command -v xclip >/dev/null 2>&1; then
    echo "Aviso: instala wl-clipboard (Wayland) o xclip (X11) para leer el portapapeles."
fi
if ! command -v zenity >/dev/null 2>&1; then
    echo "Aviso: zenity no está instalado; los errores solo irán a stderr y al log."
fi
echo "Reinicia Nautilus para cargar la extensión:  nautilus -q"
echo "Asegúrate de que el binario de la GUI está en PATH o define COPYFORGE_GUI_BIN"
echo "(ver README.md)."
