import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
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

// Valor inicial mientras responde `get_options` (coincide con el default Rust).
const DEFAULT_OPTIONS: GuiOptions = {
  preserve_metadata: true,
  overwrite: "Always",
  verify: "Size",
  jobs: 4,
  exclude: "",
  durability: "File",
};

// --- Datos de la maqueta (en P4.3 los provee el motor real) ---------------
const SRC_LABEL = "C:\\Descargas";
const DST_LABEL = "D:\\Backup";
const GLOBAL_PERCENT = 64;
const GLOBAL_BYTES = "202.78 MB";
const FILE_PERCENT = 86;
const CURRENT_FILE = "SitioWebTVCesp.7z";
const COMPLETED = "1 / 4 archivos completados";

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
  const [options, setOptions] = useState<GuiOptions>(DEFAULT_OPTIONS);
  const [statusMsg, setStatusMsg] = useState<string | null>(null);

  // Al montar: pide las opciones al backend (defaults del motor en P4.2).
  useEffect(() => {
    invoke<GuiOptions>("get_options")
      .then((opts) => {
        setOptions(opts);
        setStatusMsg(null);
      })
      .catch((e) => setStatusMsg(`Error al cargar opciones: ${String(e)}`));
  }, []);

  // Actualiza el estado local y lo empuja al backend.
  function applyOptions(patch: Partial<GuiOptions>) {
    const next = { ...options, ...patch };
    setOptions(next);
    invoke("set_options", { options: next }).catch((e) =>
      setStatusMsg(`Error al guardar opciones: ${String(e)}`),
    );
  }

  return (
    <div className="tf-window">
      {/* Rutas de origen/destino: solo lectura (las proveerá el motor en P4.3) */}
      <div className="tf-paths">
        <div className="tf-path">
          <span className="tf-path-label">Desde:</span>
          <span className="tf-path-value" title={SRC_LABEL}>
            {SRC_LABEL}
          </span>
        </div>
        <div className="tf-path">
          <span className="tf-path-label">Hacia:</span>
          <span className="tf-path-value" title={DST_LABEL}>
            {DST_LABEL}
          </span>
        </div>
      </div>

      {/* Progreso: global + fichero en curso */}
      <div className="tf-progress">
        <div className="tf-progress-row">
          <span className="tf-percent">{GLOBAL_PERCENT}%</span>
          <div className="tf-progress-track tf-track-global">
            <div
              className="tf-progress-fill tf-fill-global"
              style={{ width: `${GLOBAL_PERCENT}%` }}
            />
          </div>
          <span className="tf-progress-total">{GLOBAL_BYTES}</span>
        </div>

        <div className="tf-progress-row">
          <span className="tf-percent-file">{FILE_PERCENT}%</span>
          <div className="tf-progress-track tf-track-file">
            <div
              className="tf-progress-fill tf-fill-file"
              style={{ width: `${FILE_PERCENT}%` }}
            />
          </div>
          <span className="tf-current" title={CURRENT_FILE}>
            {CURRENT_FILE}
          </span>
        </div>
      </div>

      {/* Acciones (maqueta, sin funcionalidad hasta P4.4) */}
      <div className="tf-actions">
        <button className="tf-btn tf-action">Pausa</button>
        <button className="tf-btn tf-action">Pasar</button>
        <button className="tf-btn tf-action">Cancelar</button>
      </div>

      {/* Barra de estado */}
      <div className="tf-statusbar">
        {COMPLETED}
        {statusMsg && <span className="tf-status-error"> ⚠ {statusMsg}</span>}
      </div>

      {/* Pestañas */}
      <div className="tf-tabs">
        <button
          className={tab === "lista" ? "tf-btn tf-tab tf-tab-active" : "tf-btn tf-tab"}
          onClick={() => setTab("lista")}
        >
          Lista de copia
        </button>
        <button
          className={tab === "errores" ? "tf-btn tf-tab tf-tab-active" : "tf-btn tf-tab"}
          onClick={() => setTab("errores")}
        >
          Informe de errores
        </button>
        <button
          className={tab === "opciones" ? "tf-btn tf-tab tf-tab-active" : "tf-btn tf-tab"}
          onClick={() => setTab("opciones")}
        >
          Opciones
        </button>
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
        {tab === "errores" && <div className="tf-empty">Sin errores.</div>}
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
  );
}
