// Tipos de la frontera CLI ↔ GUI (P4.6a).

// Modo de transferencia que viaja en `start_transfer` y en `TransferRequest`.
export type TransferMode = "copy" | "move";

// Petición de transferencia encolada por otra instancia (o por el arranque con
// argumentos). El frontend la obtiene con `drain_transfers`.
export interface TransferRequest {
  srcs: string[];
  dst: string;
  options: {
    preserve_metadata: boolean;
    overwrite: string;
    verify: string;
    jobs: number;
    exclude: string;
    durability: string;
  };
  mode: TransferMode;
}

// Payload del evento `transfer-moved` (modo `move`: borrado de orígenes).
export interface MovedPayload {
  deleted: number;
  failed: number;
}
