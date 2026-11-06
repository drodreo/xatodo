/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** When 'true', enables the in-memory mock data source (UI debug mode
   * while the server is not ready) */
  readonly VITE_USE_MOCK?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
