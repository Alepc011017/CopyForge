import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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
  const menuRef = useRef<HTMLDivElement>(null);

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

            {/* Desplegable anclado al extremo derecho de la barra de archivo */}
            <div className="tf-dropdown" ref={menuRef}>
              <button
                className="tf-btn tf-dropdown-btn"
                aria-haspopup="true"
                aria-expanded={menuOpen}
                onClick={() => setMenuOpen((open) => !open)}
              >
                <span className="tf-dropdown-icon" aria-hidden="true">
                  ☰
                </span>
                {PANEL_LABELS[tab]}
                <span className="tf-dropdown-caret" aria-hidden="true">
                  ▼
                </span>
              </button>

              {menuOpen && (
                <div className="tf-dropdown-panel">
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
