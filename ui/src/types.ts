/** A todo item (aligned with the server contract) */
export interface Todo {
  id: string;
  user_id: string;
  title: string;
  notes: string | null;
  completed: boolean;
  due_at: string | null;
  created_at: string;
  updated_at: string;
}

/** Create-todo request body (POST /api/v1/todos).
 *  A type alias instead of an interface: type aliases carry an implicit
 *  index signature, so they can be assigned directly to slsdk's RequestBody
 *  (which contains a Record<string, unknown> member); interfaces do not,
 *  and passing them to the SDK directly is type-incompatible. */
export type TodoCreateInput = {
  title: string;
  notes?: string;
  due_at?: string;
};

/** Update-todo request body (PATCH /api/v1/todos/:id; due_at = null clears
 * the due time) */
export type TodoUpdateInput = {
  title?: string;
  notes?: string;
  completed?: boolean;
  due_at?: string | null;
};

/** List filter dimension */
export type TodoFilter = 'all' | 'active' | 'completed';

/** List loading status */
export type ListStatus = 'idle' | 'loading' | 'ready' | 'error';

/** List error (unauthorized=true means 401: the shell token has expired —
 * a shell-level state anomaly) */
export interface ListError {
  message: string;
  unauthorized: boolean;
}
