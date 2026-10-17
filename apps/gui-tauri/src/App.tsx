import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  getCurrentWindow,
  PhysicalSize,
  type Theme,
} from "@tauri-apps/api/window";
import logoUrl from "../../../icon/CopyForge.svg";
import "./App.css";

type Tab = "lista" | "errores" | "opciones";

interface CopyRow {
  fuente: string;
  estado: string;
  destino: string;
}

// Espejo del DTO `GuiOptions` de Rust (src-tauri/src/lib.rs). Campos en
// snake_case para coincidir 1:1 con el serde del backend.
interface GuiOptions {
  preserve_metadata: boolean;
  overwrite: string;
  verify: string;
  jobs: number;
  exclude: string;
  durability: string;
}

// `@tauri-apps/api/core` solo funciona dentro del webview de Tauri: su `invoke`
// hace `window.__TAURI_INTERNALS__.invoke(...)`, así que fuera de Tauri (p. ej.
// `npm run dev` en un navegador normal) lanza "Cannot read properties of
// undefined (reading 'invoke')". Detectamos ese caso para avisar en vez de
// romper la UI.
const IS_TAURI =
  typeof window !== "undefined" &&
  ("__TAURI_INTERNALS__" in window || "__TAURI__" in window);

const NO_BACKEND_MSG =
  "Sin backend de Tauri: ejecuta `npm run tauri dev` (en un navegador no hay IPC). " +
  "La pantalla es solo la maqueta.";

// Chequeo defensivo antes de tocar el puente IPC: si el bundle se carga sin el
// webview de Tauri, `invoke` no es utilizable.
function hasBackend(): boolean {
  return typeof invoke === "function" && IS_TAURI;
}

type StatusMsg = { text: string; level: "warn" | "error" };

// Valor inicial mientras responde `get_options` (coincide con el default Rust).
const DEFAULT_OPTIONS: GuiOptions = {
  preserve_metadata: true,
  overwrite: "Always",
  verify: "Size",
  jobs: 4,
  exclude: "",
  durability: "File",
};

const PANEL_LABELS: Record<Tab, string> = {
  lista: "Lista de copia",
  errores: "Informe de errores",
  opciones: "Opciones",
};

// Orden de las pestañas del desplegable (estilo navegador).
const PANELS: Tab[] = ["lista", "opciones", "errores"];

// --- Datos de la maqueta (en P4.3 los provee el motor real) ---------------
const SRC_LABEL = "C:\\Descargas";
const DST_LABEL = "D:\\Backup";
const GLOBAL_PERCENT = 64;
const GLOBAL_BYTES = "202.78 MB";
const FILE_PERCENT = 86;
const CURRENT_FILE = "SitioWebTVCesp.7z";
const COMPLETED = "1 / 4 archivos completados";
const DISK_LABEL = "Disco Local (D:)";
const DISK_USAGE = 68; // dummy: en P4.3 vendrá del motor

// Panel de detalles y ventana: el panel va en el flujo (justo debajo de la barra
// de archivo), así que la ventana crece lo que ocupa el panel + esta separación.
const PANEL_GAP = 6; // px lógicos (coincide con el `margin-top` del panel en CSS)
const MIN_WINDOW_HEIGHT = 240; // px lógicos: la ventana no baja de aquí

// Tabla dummy: 4 ficheros, el 2º en curso (coherente con las barras).
const DUMMY_ROWS: CopyRow[] = [
  {
    fuente: "C:\\Descargas\\Manual.pdf",
    estado: "Completado",
    destino: "D:\\Backup\\Manual.pdf",
  },
  {
    fuente: "C:\\Descargas\\SitioWebTVCesp.7z",
    estado: "86%",
    destino: "D:\\Backup\\SitioWebTVCesp.7z",
  },
  {
    fuente: "C:\\Descargas\\Fotos.zip",
    estado: "En cola",
    destino: "D:\\Backup\\Fotos.zip",
  },
  {
    fuente: "C:\\Descargas\\Notas.txt",
    estado: "En cola",
    destino: "D:\\Backup\\Notas.txt",
  },
];

export default function App() {
  const [tab, setTab] = useState<Tab>("lista");
  const [menuOpen, setMenuOpen] = useState(false);
  const [options, setOptions] = useState<GuiOptions>(DEFAULT_OPTIONS);
  const [statusMsg, setStatusMsg] = useState<StatusMsg | null>(null);
  const [diskUsage] = useState(DISK_USAGE); // dummy (P4.2)
  const [maximized, setMaximized] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  // Altura (px lógicos) que el panel abierto añade a la ventana; al cerrarlo hay
  // que restar exactamente lo que se sumó.
  const panelDeltaRef = useRef(0);
  // Altura de la ventana con el panel cerrado (px físicos). El compositor aplica
  // los cambios de tamaño de forma asíncrona, así que recalcularla desde la
  // altura "actual" en cada toggle provoca derivas: se fija una base y solo la
  // redefine un redimensionado manual del usuario.
  const baseHeightRef = useRef<number | null>(null);
  const resizingRef = useRef(false);
  const menuOpenRef = useRef(false);

  // Al montar: pide las opciones al backend (defaults del motor en P4.2).
  useEffect(() => {
    if (!hasBackend()) {
      setStatusMsg({ text: NO_BACKEND_MSG, level: "warn" });
      return;
    }
    invoke<GuiOptions>("get_options")
      .then((opts) => {
        setOptions(opts);
        setStatusMsg(null);
      })
      .catch((e) =>
        setStatusMsg({
          text: `Error al cargar opciones: ${String(e)}`,
          level: "error",
        }),
      );
  }, []);

  // Cierra el menú al pulsar fuera o con Escape.
  useEffect(() => {
    if (!menuOpen) return;
    function onPointerDown(e: MouseEvent) {
      if (!menuRef.current?.contains(e.target as Node)) setMenuOpen(false);
    }
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") setMenuOpen(false);
    }
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [menuOpen]);

  // Modo claro/oscuro: el tema del sistema decide la clase `.dark` en <html>.
  useEffect(() => {
    const apply = (theme: Theme | null) => {
      document.documentElement.classList.toggle("dark", theme === "dark");
    };

    // Sin webview de Tauri no hay API de ventana: para poder ver la maqueta en
    // ambos temas seguimos la preferencia del navegador.
    if (!hasBackend()) {
      const mq = window.matchMedia("(prefers-color-scheme: dark)");
      const onChange = (e: MediaQueryListEvent) =>
        apply(e.matches ? "dark" : "light");
      apply(mq.matches ? "dark" : "light");
      mq.addEventListener("change", onChange);
      return () => mq.removeEventListener("change", onChange);
    }

    const win = getCurrentWindow();
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void win.theme().then(apply).catch(() => {});
    void win
      .onThemeChanged(({ payload }) => apply(payload))
      .then((un) => {
        if (disposed) un();
        else unlisten = un;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // Estado de maximizado (para el botón) y altura base de la ventana colapsada.
  useEffect(() => {
    if (!hasBackend()) return;
    const win = getCurrentWindow();
    let unlisten: (() => void) | undefined;
    let disposed = false;
    const sync = () => {
      void win.isMaximized().then(setMaximized).catch(() => {});
      // Con el panel cerrado, un redimensionado manual redefine la base.
      if (menuOpenRef.current || resizingRef.current) return;
      void win
        .innerSize()
        .then((size) => {
          baseHeightRef.current = size.height;
        })
        .catch(() => {});
    };
    sync();
    void win
      .onResized(sync)
      .then((un) => {
        if (disposed) un();
        else unlisten = un;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // La ventana crece (o se encoge) con el panel de detalles, que va en el flujo:
  // el objetivo se calcula sobre la altura base, no sobre la altura actual.
  useEffect(() => {
    menuOpenRef.current = menuOpen;
    if (!hasBackend()) return;
    const panel = panelRef.current;
    const expanding = menuOpen && panel !== null;
    const deltaCss = expanding
      ? panel.offsetHeight + PANEL_GAP
      : panelDeltaRef.current;
    if (!expanding && deltaCss <= 0) {
      panelDeltaRef.current = 0;
      return;
    }
    panelDeltaRef.current = expanding ? deltaCss : 0;

    const win = getCurrentWindow();
    resizingRef.current = true;
    void (async () => {
      try {
        const scale = await win.scaleFactor();
        const inner = await win.innerSize();
        const base = baseHeightRef.current ?? inner.height;
        baseHeightRef.current = base;
        const pixels = Math.round(deltaCss * scale);
        const height = Math.max(
          Math.round(MIN_WINDOW_HEIGHT * scale),
          expanding ? base + pixels : base,
        );
        try {
          await win.setSize(new PhysicalSize(inner.width, height));
        } catch (e) {
          // Bug conocido de Tauri 2: con `decorations: false` el `setSize` del
          // webview puede no surtir efecto; en ese caso lo hace el backend.
          await invoke("adjust_window_size", {
            expanded: expanding,
            deltaHeight: deltaCss,
          });
          console.warn("[CopyForge] setSize falló; redimensionado desde Rust:", e);
        }
      } catch (e) {
        setStatusMsg({
          text: `No se pudo redimensionar la ventana: ${String(e)}`,
          level: "warn",
        });
      } finally {
        // Margen para que el evento `onResized` de esta operación no redefina la
        // base con un valor intermedio.
        window.setTimeout(() => {
          resizingRef.current = false;
        }, 300);
      }
    })();
  }, [menuOpen, tab]);

  // Botones de la barra de título (la ventana va sin decoración del sistema).
  function windowAction(action: "minimize" | "toggleMaximize" | "close") {
    if (!hasBackend()) return;
    const win = getCurrentWindow();
    const request =
      action === "minimize"
        ? win.minimize()
        : action === "toggleMaximize"
          ? win.toggleMaximize()
          : win.close();
    void request.catch((e) =>
      setStatusMsg({
        text: `Error al operar con la ventana: ${String(e)}`,
        level: "error",
      }),
    );
  }

  // Actualiza el estado local y lo empuja al backend.
  function applyOptions(patch: Partial<GuiOptions>) {
    const next = { ...options, ...patch };
    setOptions(next);
    // Sin webview de Tauri no hay backend: la UI sigue siendo navegable.
    if (!hasBackend()) return;
    invoke("set_options", { options: next }).catch((e) =>
      setStatusMsg({
        text: `Error al guardar opciones: ${String(e)}`,
        level: "error",
      }),
    );
  }

  return (
    <div className="tf-window">
      {/* Barra de título propia: la ventana va sin decoración del sistema y se
          arrastra desde `data-tauri-drag-region`. */}
      <header className="tf-titlebar" data-tauri-drag-region>
        <span className="tf-titlebar-title" data-tauri-drag-region>
          CopyForge
        </span>
        <div className="tf-titlebar-controls">
          <button
            className="tf-titlebar-btn"
            type="button"
            title="Minimizar"
            aria-label="Minimizar"
            onClick={() => windowAction("minimize")}
          >
            —
          </button>
          <button
            className="tf-titlebar-btn"
            type="button"
            title={maximized ? "Restaurar" : "Maximizar"}
            aria-label={maximized ? "Restaurar" : "Maximizar"}
            onClick={() => windowAction("toggleMaximize")}
          >
            {maximized ? "❐" : "□"}
          </button>
          <button
            className="tf-titlebar-btn tf-titlebar-close"
            type="button"
            title="Cerrar"
            aria-label="Cerrar"
            onClick={() => windowAction("close")}
          >
            ✕
          </button>
        </div>
      </header>

      {/* Cabecera: icono 64x64 + rutas de origen/destino (solo lectura) */}
      <header className="tf-header">
        <img className="tf-logo" src={logoUrl} alt="CopyForge" width={64} height={64} />
        <div className="tf-paths">
          <div className="tf-path">
            <span className="tf-path-label">Origen:</span>
            <span className="tf-path-value" title={SRC_LABEL}>
              {SRC_LABEL}
            </span>
          </div>
          <div className="tf-path">
            <span className="tf-path-label">Destino:</span>
            <span className="tf-path-value" title={DST_LABEL}>
              {DST_LABEL}
            </span>
          </div>
        </div>
      </header>

      {/* Cuerpo: izquierda (progreso + desplegable) / derecha (controles + disco) */}
      <div className="tf-body">
        <section className="tf-col-left">
          <div className="tf-bar tf-bar-global">
            <div
              className="tf-bar-fill tf-bar-fill-global"
              style={{ width: `${GLOBAL_PERCENT}%` }}
            />
            <span className="tf-bar-text tf-bar-text-global">
              {GLOBAL_PERCENT}% · {GLOBAL_BYTES}
            </span>
          </div>

          <div className="tf-dropdown" ref={menuRef}>
            {/* Barra de archivo a ancho completo; el disparador del panel es un
                caret compacto al final del contenedor flex. */}
            <div className="tf-file-row">
              <div className="tf-bar tf-bar-file">
                <div
                  className="tf-bar-fill tf-bar-fill-file"
                  style={{ width: `${FILE_PERCENT}%` }}
                />
                <span className="tf-bar-text">
                  {CURRENT_FILE} · {FILE_PERCENT}%
                </span>
              </div>

              <button
                className="tf-btn tf-dropdown-btn"
                type="button"
                aria-haspopup="true"
                aria-expanded={menuOpen}
                aria-label={`Panel: ${PANEL_LABELS[tab]}`}
                title={`Panel: ${PANEL_LABELS[tab]}`}
                onClick={() => setMenuOpen((open) => !open)}
              >
                <span
                  className={
                    menuOpen ? "tf-dropdown-caret tf-caret-open" : "tf-dropdown-caret"
                  }
                  aria-hidden="true"
                >
                  ▼
                </span>
              </button>
            </div>

            {menuOpen && (
              <div className="tf-dropdown-panel" ref={panelRef}>
                <div className="tf-tabs" role="tablist">
                  {PANELS.map((p) => (
                    <button
                      key={p}
                      role="tab"
                      aria-selected={p === tab}
                      className={p === tab ? "tf-tab tf-tab-active" : "tf-tab"}
                      onClick={() => setTab(p)}
                    >
                      {PANEL_LABELS[p]}
                    </button>
                  ))}
                </div>

                <div className="tf-tabpanel">
                  {tab === "lista" && (
                    <table className="tf-table">
                      <thead>
                        <tr>
                          <th>Fuente</th>
                          <th>Estado</th>
                          <th>Destino</th>
                        </tr>
                      </thead>
                      <tbody>
                        {DUMMY_ROWS.map((r) => (
                          <tr key={r.fuente}>
                            <td>{r.fuente}</td>
                            <td>{r.estado}</td>
                            <td>{r.destino}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  )}

                  {tab === "errores" && (
                    <div className="tf-empty">Sin errores.</div>
                  )}

                  {tab === "opciones" && (
                    <div className="tf-options">
                      <label className="tf-check">
                        <input
                          type="checkbox"
                          checked={options.preserve_metadata}
                          onChange={(e) =>
                            applyOptions({ preserve_metadata: e.target.checked })
                          }
                        />
                        Preserve metadata
                      </label>

                      <label className="tf-optrow">
                        <span className="tf-optlabel">Overwrite:</span>
                        <select
                          value={options.overwrite}
                          onChange={(e) => applyOptions({ overwrite: e.target.value })}
                        >
                          <option>Never</option>
                          <option>IfNewer</option>
                          <option>IfDifferent</option>
                          <option>Always</option>
                        </select>
                      </label>

                      <label className="tf-optrow">
                        <span className="tf-optlabel">Verify:</span>
                        <select
                          value={options.verify}
                          onChange={(e) => applyOptions({ verify: e.target.value })}
                        >
                          <option>None</option>
                          <option>Size</option>
                          <option>Hash</option>
                        </select>
                      </label>

                      <label className="tf-optrow">
                        <span className="tf-optlabel">Jobs:</span>
                        <input
                          className="tf-input tf-jobs"
                          type="number"
                          min={1}
                          max={16}
                          value={options.jobs}
                          onChange={(e) =>
                            applyOptions({
                              jobs: Math.min(16, Math.max(1, Number(e.target.value) || 1)),
                            })
                          }
                        />
                      </label>

                      <label className="tf-optrow">
                        <span className="tf-optlabel">Exclude:</span>
                        <input
                          className="tf-input"
                          type="text"
                          placeholder="*.log, target/**…"
                          value={options.exclude}
                          onChange={(e) => applyOptions({ exclude: e.target.value })}
                        />
                      </label>

                      <label className="tf-optrow">
                        <span className="tf-optlabel">Durability:</span>
                        <select
                          value={options.durability}
                          onChange={(e) => applyOptions({ durability: e.target.value })}
                        >
                          <option>None</option>
                          <option>File</option>
                          <option>Full</option>
                        </select>
                      </label>
                    </div>
                  )}
                </div>
              </div>
            )}
          </div>
        </section>

        <section className="tf-col-right">
          <button className="tf-btn tf-action">Pausa</button>
          <button className="tf-btn tf-action">Pasar</button>

          <div className="tf-disk">
            <span className="tf-disk-label">{DISK_LABEL}</span>
            <div className="tf-disk-track">
              <div className="tf-disk-fill" style={{ width: `${diskUsage}%` }} />
            </div>
          </div>

          <button className="tf-btn tf-action">Cancelar</button>
        </section>
      </div>

      {/* Barra de estado */}
      <div className="tf-statusbar">
        {COMPLETED}
        {statusMsg && (
          <span
            className={
              statusMsg.level === "warn" ? "tf-status-warn" : "tf-status-error"
            }
          >
            {" "}
            ⚠ {statusMsg.text}
          </span>
        )}
      </div>
    </div>
  );
}
