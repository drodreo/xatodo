/**
 * Component build entry (vite rollup input): default-exports the component
 * root.
 * The shell's module tier dynamically imports this artifact via ComponentHost
 * (fixed name entry.js, statically declared by manifest.ui.entry). Styles
 * are injected inside App via ?inline; this file has zero side effects.
 */
export { default } from './App';
