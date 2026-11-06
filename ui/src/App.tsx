/**
 * Component root (mount entry for the superlive shell's module tier):
 * - The build artifact is dynamically imported by the shell's ComponentHost,
 *   and this component is the default export (mount contract:
 *   superlive/shell/src/ComponentHost.tsx); main.tsx reuses this
 *   component for standalone debugging.
 * - Shell env: window.__SL_SHELL__ = { token, origin } (injected by the
 *   shell's App.tsx at startup). When missing, an explicit notice page is
 *   rendered instead of failing silently. The shell-ready-state hook
 *   (useShellState) lives in @shell/sdk/ui (the SDK version was extracted
 *   from this file's original inline implementation; the 200ms×10 polling
 *   semantics are unchanged).
 * - Styles: domain styles are fully self-contained (global.css/todo.css
 *   travel inside the bundle via ?inline, injected on mount and removed on
 *   unmount); the SDK ui facility (the .sl-ui-* class family) is not
 *   consumed, so useUiCss is not imported — importing it would inject
 *   shared-facility styles with zero consumers into the host document. All
 *   rules are scoped under the .xatodo-app prefix (see the header comment in
 *   styles/global.css).
 */
import { useEffect } from 'react';
import { useShellState } from '@shell/sdk/ui';
import { TodoPage } from './pages/TodoPage';
import globalCss from './styles/global.css?inline';
import todoCss from './styles/todo.css?inline';

/** Mount props passed through by the shell (xatodo is a single page with no
 * routing; the catch-all signature keeps the ComponentHost contract) */
export interface AppProps {
  route?: string;
}

/** Shell-env-missing notice: the component must run inside the superlive
 * shell */
function MissingShellNotice() {
  return (
    <div className="xatodo-app xatodo-missing-shell">
      <div className="xatodo-missing-card">
        <h1 className="xatodo-missing-title">XaTodo</h1>
        <p className="xatodo-missing-text">需要在 superlive 壳内运行（缺少壳注入的运行环境）。</p>
      </div>
    </div>
  );
}

export default function App(_props: AppProps) {
  // Styles enter/leave the host document with the component lifecycle and do
  // not stay in head (idempotent even under StrictMode double execution)
  useEffect(() => {
    const style = document.createElement('style');
    style.textContent = `${globalCss}\n${todoCss}`;
    document.head.appendChild(style);
    return () => style.remove();
  }, []);

  const shell = useShellState();
  if (shell === 'missing') {
    return <MissingShellNotice />;
  }
  if (shell === 'waiting') {
    return null;
  }

  return (
    <div className="xatodo-app">
      <header className="xatodo-header">
        <span className="xatodo-brand">XaTodo</span>
      </header>
      <main className="xatodo-main">
        <TodoPage />
      </main>
    </div>
  );
}
