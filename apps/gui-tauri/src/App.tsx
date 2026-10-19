import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  getCurrentWindow,
  PhysicalSize,
  type Theme,
} from "@tauri-apps/api/window";
import logoUrl from "../../../icon/CopyForge.svg";
import type { MovedPayload, TransferRequest } from "./types";
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

// Espejo del `CopyEventDto` que emite el backend en `copy-event` (src-tauri/src/lib.rs):
// un enum serializado con `#[serde(tag = "type")]`, así que el discriminador llega
// como `type` (`planned`, `fileStarted`, …) y los campos en snake_case, igual que
// `GuiOptions`: la frontera se mapea 1:1 con el serde de Rust.
type CopyEventPayload =
  | { type: "planned"; total_files: number; total_bytes: number }
  | { type: "fileStarted"; path: string; size: number }
  | { type: "progress"; path: string; bytes_copied: number; total: number }
  | { type: "fileCompleted"; path: string; elapsed_ms: number }
  | { type: "fileSkipped"; path: string; reason: string }
  | { type: "fileFailed"; path: string; error: string }
  | { type: "symlinked"; src: string; dst: string }
  | { type: "symlinkFallback"; path: string; target: string; reason: string }
  | {
      type: "finished";
      files_copied: number;
      files_skipped: number;
      files_failed: number;
      bytes_copied_total: number;
      dirs_created: number;
      symlinks_created: number;
      duration_ms: number;
      errors: string[];
      errors_truncated: number;
      symlinks_fallback: [string, string][];
    };

// Progreso de la copia real. Mientras es `null` la UI muestra los valores de la
// maqueta (barras y contadores dummy).
interface CopyProgress {
  running: boolean;
  finished: boolean;
  file: string; // nombre del fichero en curso
  fileBytes: number; // bytes copiados de ese fichero
  fileTotal: number; // tamaño de ese fichero
  filePercent: number; // 0..100
  copiedBytes: number; // bytes totales ya completados
  totalBytes: number; // bytes del plan
  globalPercent: number; // 0..100
  filesDone: number; // ficheros ya resueltos (copiados, saltados o fallados)
  filesTotal: number; // ficheros del plan
}

// Nombre base de una ruta (sirve para las rutas Windows y Unix).
function baseName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.length > 0 ? parts[parts.length - 1] : path;
}

// Tamaño en bytes con unidades legibles para las barras.
function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = n / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 100 ? 0 : 1)} ${units[unit]}`;
}

const PANEL_LABELS: Record<Tab, string> = {
  lista: "Lista de copia",
  errores: "Informe de errores",
  opciones: "Opciones",
};

// Orden de las pestañas del desplegable (estilo navegador).
const PANELS: Tab[] = ["lista", "opciones", "errores"];

// Etiquetas en español de los valores de `GuiOptions`: la clave es lo que viaja al
// backend (inglés, tal cual lo espera el serde de Rust) y el valor es lo que ve el
// usuario. Traducir solo la etiqueta (<option>/label), nunca el `value`.
const OVERWRITE_LABELS: Record<string, string> = {
  Never: "Nunca",
  IfNewer: "Si es más nuevo",
  IfDifferent: "Si es diferente",
  Always: "Siempre",
};

const VERIFY_LABELS: Record<string, string> = {
  None: "Ninguna",
  Size: "Tamaño",
  Hash: "Hash",
};

const DURABILITY_LABELS: Record<string, string> = {
  None: "Ninguna",
  File: "Archivo",
  Full: "Completa",
};

// --- Datos de la maqueta (en P4.3 los provee el motor real) ---------------
const SRC_LABEL = "C:\\Descargas";
const DST_LABEL = "D:\\Backup";
const GLOBAL_PERCENT = 64;
const GLOBAL_BYTES = "202.78 MB";
const FILE_PERCENT = 86;
const CURRENT_FILE = "SitioWebTVCesp.7z";
const FILE_PROGRESS = "1 / 4"; // dummy: en P4.3 vendrá del motor
const DISK_LABEL = "Disco Local (D:)";
const DISK_USAGE = 68; // dummy: en P4.3 vendrá del motor

// Ventana: el tamaño base es compacto (coincide con `tauri.conf.json`) y al
// abrir el panel de detalles la ventana crece hacia abajo lo que ocupa el panel
// más esta separación.
const BASE_WINDOW_WIDTH = 800; // px lógicos
const BASE_WINDOW_HEIGHT = 250; // px lógicos (coincide con `tauri.conf.json`)
const PANEL_GAP = 6; // px lógicos (coincide con el `margin-top` del panel en CSS)
const MIN_WINDOW_HEIGHT = 250; // px lógicos: la ventana no baja de aquí

// Tabla dummy: 16 ficheros, el 1º completado y el 2º en curso (coherente con las
// barras). En P4.3 las filas las provee el motor.
const DUMMY_FILES = [
  "Manual.pdf",
  "SitioWebTVCesp.7z",
  "Fotos.zip",
  "Notas.txt",
  "Presupuesto2026.xlsx",
  "Planos_v3.dwg",
  "Capturas.zip",
  "Musica.7z",
  "Informe_final.docx",
  "Logo.svg",
  "Videos_webm.7z",
  "BaseDatos.sql",
  "Plantilla.pptx",
  "Certificados.p12",
  "Backup_old.zip",
  "README.md",
];

const DUMMY_ROWS: CopyRow[] = DUMMY_FILES.map((name, i) => ({
  fuente: `C:\\Descargas\\${name}`,
  estado: i === 0 ? "Completado" : i === 1 ? `${FILE_PERCENT}%` : "En cola",
  destino: `D:\\Backup\\${name}`,
}));

export default function App() {
  const [tab, setTab] = useState<Tab>("lista");
  const [menuOpen, setMenuOpen] = useState(false);
  const [options, setOptions] = useState<GuiOptions>(DEFAULT_OPTIONS);
  const [statusMsg, setStatusMsg] = useState<StatusMsg | null>(null);
  const [diskUsage] = useState(DISK_USAGE); // dummy (P4.2)
  const [maximized, setMaximized] = useState(false);
  // Rutas de la copia (editables en P4.3 para poder lanzar copias reales; en P4.4
  // llegan los selectores de carpeta) y progreso real del motor.
  const [src, setSrc] = useState(SRC_LABEL);
  const [dst, setDst] = useState(DST_LABEL);
  const [copyProgress, setCopyProgress] = useState<CopyProgress | null>(null);
  // Pausa/reanudar (P4.4): el flag vive en `CopyState` y lo mueve `pause_copy`.
  const [paused, setPaused] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  // Altura (px lógicos) que el panel abierto añade a la ventana; sirve para no
  // redimensionar cuando el panel ya estaba cerrado.
  const panelDeltaRef = useRef(0);

  // Cancelación (P4.4): `copy-cancelled` marca que el **próximo** `finished` es
  // de cancelación. Se resetea al arrancar cada copia para que un aviso tardío no
  // marque la siguiente.
  const cancelledRef = useRef(false);

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

  // Estado de maximizado, para que el botón de la barra de título lo refleje.
  useEffect(() => {
    if (!hasBackend()) return;
    const win = getCurrentWindow();
    let unlisten: (() => void) | undefined;
    let disposed = false;
    const sync = () => {
      void win.isMaximized().then(setMaximized).catch(() => {});
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

  // Progreso real (P4.3): el backend retransmite los `CopyEvent` del motor como
  // eventos `copy-event`. `listen` devuelve un unlisten que se libera al desmontar.
  useEffect(() => {
    if (!hasBackend()) return;
    let unlisten: UnlistenFn | undefined;
    let disposed = false;
    void listen<CopyEventPayload>("copy-event", (event) =>
      applyCopyEvent(event.payload),
    )
      .then((un) => {
        if (disposed) un();
        else unlisten = un;
      })
      .catch((e) =>
        setStatusMsg({
          text: `No se pudo escuchar 'copy-event': ${String(e)}`,
          level: "error",
        }),
      );
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // Aviso de cancelación (P4.4): lo emite `cancel_copy` y marca que el próximo
  // `finished` es de cancelación. Es un flag que se consume (y se resetea) en el
  // caso "finished" de `applyCopyEvent`.
  useEffect(() => {
    if (!hasBackend()) return;
    let unlisten: UnlistenFn | undefined;
    let disposed = false;
    void listen("copy-cancelled", () => {
      cancelledRef.current = true;
    })
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

  // Transferencias encoladas por la CLI (P4.6a). El listener de `copy-request` se
  // registra en cada montaje (StrictMode lo des-registra en la limpieza), y el
  // guard `drainedRef` solo cubre el drenado inicial con argv.
  const drainedRef = useRef(false);
  useEffect(() => {
    if (!hasBackend()) return;
    let unlisten: UnlistenFn | undefined;
    let disposed = false;
    const drain = async () => {
      const pending = await invoke<TransferRequest[]>("drain_transfers");
      for (const t of pending) {
        await invoke("start_transfer", {
          srcs: t.srcs,
          dst: t.dst,
          options: t.options,
          mode: t.mode,
        }).catch((e) =>
          setStatusMsg({
            text: `No se pudo iniciar una transferencia encolada: ${String(e)}`,
            level: "error",
          }),
        );
      }
    };
    void listen("copy-request", () => {
      void drain();
    })
      .then((un) => {
        if (disposed) un();
        else unlisten = un;
      })
      .catch(() => {});
    // Arranque con argumentos: la petición ya está en `PendingCommands`.
    if (!drainedRef.current) {
      drainedRef.current = true;
      void drain();
    }
    // Resultado del borrado de orígenes en modo `move`.
    void listen<MovedPayload>("transfer-moved", (ev) => {
      setStatusMsg({
        text:
          ev.payload.failed > 0
            ? `Move: ${ev.payload.deleted} orígenes borrados, ${ev.payload.failed} sin poder borrar`
            : `Move: ${ev.payload.deleted} orígenes borrados`,
        level: "warn",
      });
    }).catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // Aplica el tamaño de la ventana: ancho base y altura = base + alto del panel
  // (cuando `expanded`). Si está maximizada y ya tiene sitio de sobra no se toca,
  // porque redimensionarla la restauraría; si está maximizada pero el contenido no
  // cabe, sí se ajusta (evita quedarse atascada por un estado maximizado obsoleto).
  async function resizeWindow(expanded: boolean, deltaCss: number) {
    if (!hasBackend()) return;
    try {
      const win = getCurrentWindow();
      const scale = await win.scaleFactor();
      const base = Math.round(BASE_WINDOW_HEIGHT * scale);
      const pixels = Math.round(deltaCss * scale);
      const height = Math.max(
        Math.round(MIN_WINDOW_HEIGHT * scale),
        expanded ? base + pixels : base,
      );
      const inner = await win.innerSize();
      if ((await win.isMaximized()) && inner.height >= height) return;
      try {
        await win.setSize(
          new PhysicalSize(Math.round(BASE_WINDOW_WIDTH * scale), height),
        );
      } catch (e) {
        // Bug conocido de Tauri 2: con `decorations: false` el `setSize` del
        // webview puede no surtir efecto; en ese caso lo hace el backend.
        await invoke("adjust_window_size", { expanded, deltaHeight: deltaCss });
        console.warn("[CopyForge] setSize falló; redimensionado desde Rust:", e);
      }
    } catch (e) {
      setStatusMsg({
        text: `No se pudo redimensionar la ventana: ${String(e)}`,
        level: "warn",
      });
    }
  }

  // Arranque: fija la base 800x250 aunque el gestor de ventanas recuerde otra
  // geometría para el mismo `identifier`.
  useEffect(() => {
    void resizeWindow(false, 0);
  }, []);

  // La ventana crece (o se encoge) con el panel de detalles, que va en el flujo:
  // el objetivo es siempre una altura absoluta sobre la base compacta (la ventana
  // no se redimensiona a mano, así que la base no cambia).
  useEffect(() => {
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
    void resizeWindow(expanding, deltaCss);
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

  // Actualiza la UI a partir de un evento del motor (P4.3). Los porcentajes se
  // recalculan con los bytes: la global usa `copiedBytes / totalBytes` y la del
  // fichero `fileBytes / fileTotal`.
  function applyCopyEvent(ev: CopyEventPayload) {
    switch (ev.type) {
      case "planned":
        setCopyProgress({
          running: true,
          finished: false,
          file: "",
          fileBytes: 0,
          fileTotal: 0,
          filePercent: 0,
          copiedBytes: 0,
          totalBytes: ev.total_bytes,
          globalPercent: 0,
          filesDone: 0,
          filesTotal: ev.total_files,
        });
        break;

      case "fileStarted":
        setCopyProgress((p) => ({
          ...(p ?? {
            running: true,
            finished: false,
            fileBytes: 0,
            fileTotal: 0,
            filePercent: 0,
            copiedBytes: 0,
            totalBytes: 0,
            globalPercent: 0,
            filesDone: 0,
            filesTotal: 0,
          }),
          running: true,
          file: baseName(ev.path),
          fileBytes: 0,
          fileTotal: ev.size,
          filePercent: 0,
        }));
        break;

      case "progress":
        setCopyProgress((p) => {
          if (!p) return p;
          return {
            ...p,
            fileBytes: ev.bytes_copied,
            filePercent: p.fileTotal > 0 ? (ev.bytes_copied / p.fileTotal) * 100 : 0,
            globalPercent:
              p.totalBytes > 0 ? (p.copiedBytes / p.totalBytes) * 100 : 0,
          };
        });
        break;

      case "fileCompleted":
        setCopyProgress((p) => {
          if (!p) return p;
          const filesDone = p.filesDone + 1;
          // El tamaño se aprende en `fileStarted`; aquí solo se acumula.
          const copiedBytes = p.copiedBytes + p.fileTotal;
          return {
            ...p,
            filesDone,
            copiedBytes,
            filePercent: 100,
            globalPercent:
              p.totalBytes > 0 ? (copiedBytes / p.totalBytes) * 100 : 0,
          };
        });
        break;

      case "fileSkipped":
      case "fileFailed":
        setCopyProgress((p) => (p ? { ...p, filesDone: p.filesDone + 1 } : p));
        if (ev.type === "fileFailed") {
          setStatusMsg({ text: `${baseName(ev.path)}: ${ev.error}`, level: "error" });
        }
        break;

      case "finished": {
        // El flag se lee y se resetea aquí: un `copy-cancelled` previo convierte
        // este `finished` en una cancelación.
        const fueCancelado = cancelledRef.current;
        cancelledRef.current = false;
        // P4.4 (matices A): al terminar no puede quedar la pausa activa, aunque la
        // copia se cancelara estando pausada.
        setPaused(false);
        setCopyProgress((p) =>
          p
            ? {
                ...p,
                running: false,
                finished: true,
                filePercent: 100,
                globalPercent: 100,
                copiedBytes: ev.bytes_copied_total,
              }
            : p,
        );
        const errores =
          ev.files_failed > 0 ? `, ${ev.files_failed} con errores` : "";
        const omitidos =
          ev.errors_truncated > 0
            ? `, ${ev.errors_truncated} errores adicionales omitidos (total: ${
                ev.errors.length + ev.errors_truncated
              })`
            : "";
        const marca = fueCancelado ? "Cancelado" : "Copia terminada";
        setStatusMsg({
          text: `${marca}: ${ev.files_copied} ficheros, ${formatBytes(
            ev.bytes_copied_total,
          )} en ${(ev.duration_ms / 1000).toFixed(1)} s${errores}${omitidos}`,
          level: "warn",
        });
        break;
      }

      default:
        break;
    }
  }

  // Lanza una copia real (P4.3) con las rutas de la cabecera y las opciones
  // actuales. El progreso llega después por `copy-event`.
  function startCopy() {
    if (!hasBackend() || (copyProgress?.running ?? false)) return;
    // Tras el guard: si no se arranca nada, no tocamos el flag.
    cancelledRef.current = false;
    setCopyProgress(null);
    setStatusMsg(null);
    invoke("start_transfer", { srcs: [src], dst, options, mode: "copy" }).catch((e) =>
      setStatusMsg({
        text: `No se pudo iniciar la copia: ${String(e)}`,
        level: "error",
      }),
    );
  }

  // Cancela la copia en curso (P4.3). Pausa/Saltar llegan en P4.4.
  function cancelCopy() {
    if (!hasBackend() || !(copyProgress?.running ?? false)) return;
    invoke("cancel_copy").catch((e) =>
      setStatusMsg({
        text: `No se pudo cancelar la copia: ${String(e)}`,
        level: "error",
      }),
    );
  }

  // Pausa/reanuda la copia (P4.4): el flag vive en `CopyState`.
  function togglePause() {
    if (!hasBackend()) return;
    const next = !paused;
    setPaused(next);
    invoke("pause_copy", { paused: next }).catch((e) =>
      setStatusMsg({
        text: `No se pudo cambiar la pausa: ${String(e)}`,
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

  // Valores mostrados: los de la maqueta en reposo y los reales del motor en curso.
  const progress = copyProgress;
  const running = progress?.running ?? false;
  const globalPercent = progress
    ? Math.round(progress.globalPercent)
    : GLOBAL_PERCENT;
  const globalText = progress
    ? `${formatBytes(progress.copiedBytes)} de ${formatBytes(progress.totalBytes)}`
    : GLOBAL_BYTES;
  const currentFile =
    progress && progress.file ? progress.file : CURRENT_FILE;
  const filePercent = progress ? Math.round(progress.filePercent) : FILE_PERCENT;

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

      {/* Cabecera: icono 64x64 + rutas de origen/destino (editables en P4.3 para
          poder lanzar copias reales; en P4.4 llegan los selectores de carpeta). */}
      <header className="tf-header">
        <img className="tf-logo" src={logoUrl} alt="CopyForge" width={64} height={64} />
        <div className="tf-paths">
          <div className="tf-path">
            <span className="tf-path-label">Origen:</span>
            <input
              className="tf-input tf-path-input"
              value={src}
              onChange={(e) => setSrc(e.target.value)}
              aria-label="Origen"
              spellCheck={false}
            />
          </div>
          <div className="tf-path">
            <span className="tf-path-label">Destino:</span>
            <input
              className="tf-input tf-path-input"
              value={dst}
              onChange={(e) => setDst(e.target.value)}
              aria-label="Destino"
              spellCheck={false}
            />
          </div>
          <div className="tf-path">
            <span className="tf-path-label">Fichero:</span>
            <span className="tf-path-value">
              {progress
                ? `${progress.filesDone} / ${progress.filesTotal}`
                : FILE_PROGRESS}
            </span>
          </div>
        </div>

        {/* Botón temporal de P4.3: lanza la copia con las rutas de la cabecera.
            En P4.4 se integra con Pausa/Saltar y los selectores de carpeta. */}
        <button
          className="tf-btn tf-action tf-copy-start"
          type="button"
          onClick={startCopy}
          disabled={running}
          title="Iniciar copia con las rutas de la cabecera"
        >
          Iniciar copia
        </button>
      </header>

      {/* Cuerpo: izquierda (progreso + desplegable) / derecha (controles + disco) */}
      <div className="tf-body">
        <section className="tf-col-left">
          <div className="tf-bar tf-bar-global">
            <div
              className="tf-bar-fill tf-bar-fill-global"
              style={{ width: `${globalPercent}%` }}
            />
            <span className="tf-bar-text tf-bar-text-global">
              {globalPercent}% · {globalText}
            </span>
          </div>

          <div className="tf-dropdown" ref={menuRef}>
            {/* Barra de archivo a ancho completo; el disparador del panel es un
                caret compacto al final del contenedor flex. */}
            <div className="tf-file-row">
              <div className="tf-bar tf-bar-file">
                <div
                  className="tf-bar-fill tf-bar-fill-file"
                  style={{ width: `${filePercent}%` }}
                />
                <span className="tf-bar-text">
                  {currentFile} · {filePercent}%
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
                    <div className="tf-table-wrap">
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
                    </div>
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
                        Preservar metadatos
                      </label>

                      <label className="tf-optrow">
                        <span className="tf-optlabel">Sobrescribir:</span>
                        <select
                          value={options.overwrite}
                          onChange={(e) => applyOptions({ overwrite: e.target.value })}
                        >
                          {Object.entries(OVERWRITE_LABELS).map(([value, text]) => (
                            <option key={value} value={value}>
                              {text}
                            </option>
                          ))}
                        </select>
                      </label>

                      <label className="tf-optrow">
                        <span className="tf-optlabel">Verificar:</span>
                        <select
                          value={options.verify}
                          onChange={(e) => applyOptions({ verify: e.target.value })}
                        >
                          {Object.entries(VERIFY_LABELS).map(([value, text]) => (
                            <option key={value} value={value}>
                              {text}
                            </option>
                          ))}
                        </select>
                      </label>

                      <label className="tf-optrow">
                        <span className="tf-optlabel">Hilos simultáneos:</span>
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
                        <span className="tf-optlabel">Excluir:</span>
                        <input
                          className="tf-input"
                          type="text"
                          placeholder="*.log, target/**"
                          value={options.exclude}
                          onChange={(e) => applyOptions({ exclude: e.target.value })}
                        />
                      </label>

                      <label className="tf-optrow">
                        <span className="tf-optlabel">Durabilidad:</span>
                        <select
                          value={options.durability}
                          onChange={(e) => applyOptions({ durability: e.target.value })}
                        >
                          {Object.entries(DURABILITY_LABELS).map(([value, text]) => (
                            <option key={value} value={value}>
                              {text}
                            </option>
                          ))}
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
          {/* Pausa/reanudar (P4.4). Saltar sigue deshabilitado: necesita un handle
              por tarea que el motor todavía no expone. */}
          <button
            className="tf-btn tf-action"
            type="button"
            onClick={togglePause}
            disabled={!running && !paused}
            title={paused ? "Reanudar la copia" : "Pausar la copia"}
          >
            {paused ? "Reanudar" : "Pausa"}
          </button>
          <button
            className="tf-btn tf-action"
            type="button"
            disabled
            title="Saltar (pendiente: requiere handle por tarea)"
          >
            Saltar
          </button>

          <div className="tf-disk">
            <span className="tf-disk-label">{DISK_LABEL}</span>
            <div className="tf-disk-track">
              <div className="tf-disk-fill" style={{ width: `${diskUsage}%` }} />
            </div>
          </div>

          <button
            className="tf-btn tf-action"
            type="button"
            onClick={cancelCopy}
            disabled={!running}
            title="Cancelar la copia en curso"
          >
            Cancelar
          </button>
        </section>
      </div>

      {/* Barra de estado: solo se muestra cuando hay algo que avisar */}
      {statusMsg && (
        <div className="tf-statusbar">
          <span
            className={
              statusMsg.level === "warn" ? "tf-status-warn" : "tf-status-error"
            }
            title={statusMsg.text}
          >
            ⚠ {statusMsg.text}
          </span>
        </div>
      )}
    </div>
  );
}
