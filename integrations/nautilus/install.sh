#!/usr/bin/env bash
# Instala (o desinstala) la extensión de Nautilus de CopyForge.
set -euo pipefail

SRC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEST_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/nautilus-python/extensions"
BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
BIN_NAME="copyforge-gui-tauri"
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

binario_en_path() {
    command -v copyforge-gui-tauri >/dev/null 2>&1 \
        || command -v copyforge >/dev/null 2>&1 \
        || command -v CopyForge >/dev/null 2>&1
}

enlazar_binario() {
    local destino
    destino="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
    mkdir -p "$BIN_DIR"
    ln -sfn "$destino" "$BIN_DIR/$BIN_NAME"
    echo "enlazado: $BIN_DIR/$BIN_NAME -> $destino"
    case ":$PATH:" in
        *":$BIN_DIR:"*) ;;
        *)
            echo "Aviso: $BIN_DIR no está en PATH; añádelo o exporta" \
                 "COPYFORGE_GUI_BIN con la ruta al binario."
            ;;
    esac
}

# Asegura que la extensión encontrará el binario de la GUI: PATH, la propia
# variable COPYFORGE_GUI_BIN o un enlace en ~/.local/bin.
asegurar_binario() {
    if binario_en_path; then
        echo "binario: $(command -v copyforge-gui-tauri \
            || command -v copyforge || command -v CopyForge)"
        return 0
    fi

    if [ -n "${COPYFORGE_GUI_BIN:-}" ] && [ -x "${COPYFORGE_GUI_BIN}" ]; then
        echo "binario: usando COPYFORGE_GUI_BIN=$COPYFORGE_GUI_BIN"
        return 0
    fi

    local respuesta=""
    if [ -t 0 ]; then
        read -r -p "Ruta al binario de CopyForge (Enter = buscar automáticamente): " respuesta \
            || respuesta=""
    fi
    respuesta="${respuesta/#\~/$HOME}"

    if [ -n "$respuesta" ]; then
        if [ -x "$respuesta" ]; then
            enlazar_binario "$respuesta"
        else
            echo "Aviso: '$respuesta' no es un ejecutable; no se enlaza." >&2
        fi
        return 0
    fi

    local candidatos=(
        "$SRC_DIR/../../apps/gui-tauri/src-tauri/target/release/$BIN_NAME"
        "$SRC_DIR/../../target/release/$BIN_NAME"
        "$HOME/.cargo/bin/$BIN_NAME"
        "/usr/local/bin/$BIN_NAME"
    )
    local candidato
    for candidato in "${candidatos[@]}"; do
        if [ -x "$candidato" ]; then
            enlazar_binario "$candidato"
            return 0
        fi
    done

    echo >&2
    echo "Aviso: no se encontró el binario de CopyForge." >&2
    echo "Instala el binario en PATH o exporta COPYFORGE_GUI_BIN antes de usar el menú." >&2
    echo "(Se genera con: cd apps/gui-tauri && npm run tauri build)" >&2
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
        if [ -L "$BIN_DIR/$BIN_NAME" ]; then
            rm -f "$BIN_DIR/$BIN_NAME"
            echo "eliminado: $BIN_DIR/$BIN_NAME (enlace)"
        fi
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

asegurar_binario

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
