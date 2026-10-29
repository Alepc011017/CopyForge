# Extensión de Nautilus para CopyForge

Integración de CopyForge en el menú contextual de Nautilus (GNOME Files) vía
`nautilus-python`. La extensión **no copia nada por su cuenta**: lee el
portapapeles y lanza el binario de la GUI

```
copyforge-gui-tauri copy|move <src...> --to <dst> --overwrite always --verify size
```

que encola la transferencia y la ejecuta con el motor de `crates/core`. Sin
terminal y sin scripts temporales.

## Menús

| Dónde | Entrada | Acción |
|---|---|---|
| Fondo de carpeta | **Pegar con CopyForge** | `copy` de lo copiado/cortado |
| Fondo de carpeta | **Mover aquí con CopyForge** | `move` de lo copiado/cortado |
| Selección | **Copiar con CopyForge** | escribe `copy` en el portapapeles |
| Selección | **Mover con CopyForge** | escribe `cut` en el portapapeles |

`Pegar con CopyForge` respeta el modo del portapapeles: si marcaste con
**Mover con CopyForge** (`cut`), pega y borra los orígenes al terminar una copia
limpia; si marcaste con **Copiar con CopyForge** (`copy`), solo copia.

## Requisitos

- `nautilus-python`:

  | Distro | Paquete |
  |---|---|
  | Debian / Ubuntu | `python3-nautilus` |
  | Fedora | `nautilus-python` |
  | Arch | `python-nautilus` |
  | openSUSE | `python3-nautilus` |

- Portapapeles: `wl-clipboard` (Wayland) o `xclip` (X11). Sin uno de los dos la
  extensión no puede leer ni escribir el portapapeles.
- `zenity` (opcional): muestra los errores en un diálogo. Sin él, los errores
  van a stderr y al log.
- El binario de la GUI. Se recomienda el de **release**
  (`target/release/copyforge-gui-tauri` o el del bundle), porque el de debug usa
  el `devUrl` de Vite y necesita el servidor de desarrollo arrancado.

## Instalación

```bash
cd integrations/nautilus
./install.sh          # copia la extensión a ~/.local/share/nautilus-python/extensions
nautilus -q           # reinicia Nautilus
```

`install.sh` también se ocupa del binario de la GUI: si no está en `PATH` ni en
`COPYFORGE_GUI_BIN`, pregunta la ruta (o la autodetecta en
`apps/gui-tauri/src-tauri/target/release/` y `~/.cargo/bin/`) y crea el enlace
`~/.local/bin/copyforge-gui-tauri`, que sí está en `PATH`. Con eso Nautilus lo
encuentra sin tocar el entorno de la sesión. `--uninstall` borra ese enlace.

Si prefieres no usar el enlace, exporta la ruta **en el entorno de la sesión**
(por ejemplo en `~/.profile` o `~/.config/environment.d/copyforge.conf`):

```bash
export COPYFORGE_GUI_BIN="$HOME/ruta/a/target/release/copyforge-gui-tauri"
```

Nautilus hereda el entorno del gestor de sesión, no el del shell desde el que
lo reinicies; si no lo detecta, ciérralo y vuelve a iniciarlo desde el menú de
aplicaciones.

### Desinstalar

```bash
./install.sh --uninstall
nautilus -q
```

## Uso

1. Selecciona archivos → clic derecho → **Copiar con CopyForge** (o
   **Mover con CopyForge**).
2. Ve a la carpeta destino → clic derecho en el fondo → **Pegar con CopyForge**
   (o **Mover aquí con CopyForge**).
3. La ventana de CopyForge se abre con la tarea encolada y muestra el progreso.

Se descartan los orígenes que coinciden con el destino o lo contienen (por
ejemplo, mover una carpeta dentro de sí misma), para no borrar datos.

## Solución de problemas

- **El menú no aparece**
  - Comprueba que `nautilus-python` está instalado (`./install.sh` te lo dice).
  - Reinicia Nautilus: `nautilus -q` (o cierra sesión).
  - Verifica que el fichero está en
    `~/.local/share/nautilus-python/extensions/copyforge_extension.py`
    (junto a `copyforge_ext_logic.py`).
- **"No se encontró el binario de CopyForge"**: define `COPYFORGE_GUI_BIN` con
  la ruta absoluta al ejecutable, o ponlo en `PATH`.
- **No pega / no detecta `cut`**: instala `wl-clipboard` (Wayland) o `xclip`
  (X11). El tipo `x-special/gnome-copied-files` es el que distingue copiar de
  cortar; sin él se asume copiar.
- **Log**: `~/.local/state/copyforge/nautilus.log` (o `$XDG_STATE_HOME`).
  Puedes cambiarlo con `COPYFORGE_LOG=/ruta/al.log`. La extensión también
  escribe en stderr, que Nautilus recoge en su journal.

## Tests

La lógica pura vive en `copyforge_ext_logic.py` (no importa `gi`), así que se
puede probar sin Nautilus instalado:

```bash
python3 -m unittest discover integrations/nautilus/tests
```
