/**
 * API client: component-side assembly of the A-plane contract (the superlive
 * shell's unified gateway).
 *
 * - Shell env: window.__SL_SHELL__ = { token, origin }, injected by the
 *   shell's App.tsx at startup; parsed lazily (read on the first API call),
 *   and an explicit error is thrown when missing.
 * - SDK: slsdk-ts createClient, with baseUrl pointing at this component's
 *   namespace prefix /api/todo; request paths use the component's own full
 *   contract (/api/v1/...) — after the shell strips /api/todo, the component
 *   receives its own full path, verbatim identical to standalone form.
 * - PATCH: not in the SDK's five-verb vocabulary; the A-plane protocol is
 *   open HTTP (architecture.md), and the server contract includes PATCH, so
 *   this sends PATCH directly via native fetch under the open protocol
 *   (token injected manually).
 */
import { createClient, ApiError, type RequestBody } from '@shell/sdk';

export { ApiError };

/** Component runtime environment injected by the shell (written by
 * superlive/shell/src/App.tsx at startup) */
interface ShellEnv {
  /** Origin of the shell-embedded server endpoint (http://127.0.0.1:39876) */
  origin: string;
  /** Shell-level token (per-component credential issued when the shell
   * spawns the component) */
  token: string;
}

declare global {
  interface Window {
    __SL_SHELL__?: ShellEnv;
  }
}

/** This component's namespace prefix on the shell gateway (the shell strips
 * the prefix before forwarding to the component backend) */
const NAMESPACE_PREFIX = '/api/todo';

/** Reads the shell env; missing env is an environment error (the component
 * must run inside the shell, no silent fallback) */
function requireShellEnv(): ShellEnv {
  const env = window.__SL_SHELL__;
  if (!env) {
    throw new Error('缺少壳环境（window.__SL_SHELL__）：XaTodo 必须运行在 superlive 壳内');
  }
  return env;
}

type SdkClient = ReturnType<typeof createClient>;

/** SDK client singleton: created on first call (token/origin are
 * shell-level singletons, snapshot-safe) */
let sdkClient: SdkClient | null = null;
function sdk(): SdkClient {
  sdkClient ??= createClient({
    baseUrl: `${requireShellEnv().origin}${NAMESPACE_PREFIX}`,
    token: requireShellEnv().token,
  });
  return sdkClient;
}

interface RequestOptions {
  method?: 'GET' | 'POST' | 'PATCH' | 'DELETE';
  body?: unknown;
  /** Query params appended to the URL (natively passed through by the SDK;
   * not supported on the direct-fetch PATCH path) */
  query?: Record<string, string>;
}

/** PATCH path: direct open-HTTP fetch (the SDK's five verbs exclude PATCH,
 * see the file header) */
async function patchViaFetch<T>(path: string, options: RequestOptions): Promise<T> {
  const env = requireShellEnv();
  const headers: Record<string, string> = { Authorization: `Bearer ${env.token}` };
  if (options.body !== undefined) headers['Content-Type'] = 'application/json';
  const res = await fetch(`${env.origin}${NAMESPACE_PREFIX}${path}`, {
    method: 'PATCH',
    headers,
    body: options.body !== undefined ? JSON.stringify(options.body) : undefined,
  });
  if (!res.ok) throw await ApiError.from(res);
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

/** Issues an API request; 204 resolves to undefined, anything else resolves
 * to the parsed JSON */
export async function apiFetch<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const method = options.method ?? 'GET';
  try {
    if (method === 'PATCH') {
      return await patchViaFetch<T>(path, options);
    }
    const res = await sdk().request(
      method,
      path,
      options.body as RequestBody | undefined,
      options.query ? { query: options.query } : undefined,
    );
    if (res.status === 204) return undefined as T;
    return (await res.json()) as T;
  } catch (err) {
    if (err instanceof ApiError) throw err;
    // Network-layer failures (offline / shell service not started / shell
    // env missing) are normalized into a connection-class error
    const reason = err instanceof Error ? err.message : '未知错误';
    throw new ApiError(`网络请求失败：${reason}`, 0, undefined);
  }
}
