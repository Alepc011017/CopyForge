import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import "./App.css";

type Tab = "lista" | "errores" | "opciones";

interface CopyRow {
  fuente: string;
  estado: string;
  destino: string;
}

// Tabla dummy: 4 ficheros, el 2º en curso (coherente con "1 / 4" y el 64%).
const DUMMY_ROWS: CopyRow[] = [
  {
    fuente: "C:\\Descargas\\Manual.pdf",
    estado: "OK",
    destino: "D:\\Backup\\Manual.pdf",
  },
  {
    fuente: "C:\\Descargas\\SitioWebTVCesp.7z",
    estado: "64%",
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

async function pickInto(setter: (v: string) => void, directory: boolean) {
  const sel = await open({ directory, multiple: false });
  if (typeof sel === "string") setter(sel);
}

export default function App() {
  const [origen, setOrigen] = useState("");
  const [destino, setDestino] = useState("");
  const [tab, setTab] = useState<Tab>("lista");
  // Opciones (espejo de GuiOptions, solo estado local).
  const [preserve, setPreserve] = useState(true);
  const [overwrite, setOverwrite] = useState("Always");
  const [verify, setVerify] = useState("Size");
  const [jobs, setJobs] = useState(4);
  const [exclude, setExclude] = useState("");
  const [durability, setDurability] = useState("File");

  return (
    <div className="tf-window">
      {/* a) Origen / destino */}
      <div className="tf-row">
        <label className="tf-label">Desde:</label>
        <input
          className="tf-input"
          type="text"
          value={origen}
          onChange={(e) => setOrigen(e.target.value)}
        />
        <button className="tf-btn" onClick={() => void pickInto(setOrigen, true)}>
          Carpeta…
        </button>
        <button className="tf-btn" onClick={() => void pickInto(setOrigen, false)}>
          Archivo…
        </button>
      </div>
      <div className="tf-row">
        <label className="tf-label">Hacia:</label>
        <input
          className="tf-input"
          type="text"
          value={destino}
          onChange={(e) => setDestino(e.target.value)}
        />
        <button className="tf-btn" onClick={() => void pickInto(setDestino, true)}>
          Carpeta…
        </button>
      </div>

      {/* b) Progreso grande */}
      <div className="tf-progress-zone">
        <div className="tf-percent">64%</div>
        <div className="tf-progress-track">
          <div className="tf-progress-fill" style={{ width: "64%" }} />
        </div>
        <div className="tf-current">SitioWebTVCesp.7z</div>
      </div>

      {/* c) Botones (maqueta, sin funcionalidad) */}
      <div className="tf-row tf-actions">
        <button className="tf-btn tf-action">Pausa</button>
        <button className="tf-btn tf-action">Pasar</button>
        <button className="tf-btn tf-action">Cancelar</button>
      </div>

      {/* d) Barra de estado */}
      <div className="tf-statusbar">1 / 4 · Total: 202.78 MB</div>

      {/* e) Pestañas */}
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
        {tab === "errores" && (
          <div className="tf-empty">Sin errores.</div>
        )}
        {tab === "opciones" && (
          <div className="tf-options">
            <label className="tf-check">
              <input
                type="checkbox"
                checked={preserve}
                onChange={(e) => setPreserve(e.target.checked)}
              />
              Preserve metadata
            </label>
            <label className="tf-optrow">
              Overwrite:
              <select value={overwrite} onChange={(e) => setOverwrite(e.target.value)}>
                <option>Never</option>
                <option>IfNewer</option>
                <option>IfDifferent</option>
                <option>Always</option>
              </select>
            </label>
            <label className="tf-optrow">
              Verify:
              <select value={verify} onChange={(e) => setVerify(e.target.value)}>
                <option>None</option>
                <option>Size</option>
                <option>Hash</option>
              </select>
            </label>
            <label className="tf-optrow">
              Jobs:
              <input
                className="tf-input tf-jobs"
                type="number"
                min={1}
                max={16}
                value={jobs}
                onChange={(e) => setJobs(Number(e.target.value) || 1)}
              />
            </label>
            <label className="tf-optrow">
              Exclude:
              <input
                className="tf-input"
                type="text"
                placeholder="*.log, target/**…"
                value={exclude}
                onChange={(e) => setExclude(e.target.value)}
              />
            </label>
            <label className="tf-optrow">
              Durability:
              <select value={durability} onChange={(e) => setDurability(e.target.value)}>
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
