/**
 * Service interface definitions: the real implementation and the mock
 * implementation share the same interface; switching happens only at the
 * assembly entry.
 */
import type { Observable } from 'rxjs';
import type {
  ListError,
  ListStatus,
  Todo,
  TodoCreateInput,
  TodoFilter,
  TodoUpdateInput,
} from '../types';

/** Todo service interface */
export interface TodoService {
  /** List state stream: what the server returned under the current filter
   * (the single authoritative list) */
  readonly todos$: Observable<Todo[]>;
  /** List loading-status stream */
  readonly listStatus$: Observable<ListStatus>;
  /** List error stream (null means no error) */
  readonly listError$: Observable<ListError | null>;
  /** Filter state stream */
  readonly filter$: Observable<TodoFilter>;
  /**
   * Display stream: todos$ re-filtered client-side by the current filter.
   * Double safety net with server-side filtering; the display stays
   * self-consistent during optimistic-update windows/rollbacks.
   */
  readonly filteredTodos$: Observable<Todo[]>;
  /** Switches the filter (automatically re-fetches internally) */
  setFilter(filter: TodoFilter): void;
  /** Fetches the list; errors are consumed inside the stream (funneled into
   * listError$), subscribers need no handling */
  refresh(): Observable<void>;
  /** Creates a todo, refreshes the list automatically on success */
  create(input: TodoCreateInput): Observable<Todo>;
  /** Updates a todo (optimistic update; automatic rollback and re-fetch on
   * failure) */
  update(id: string, patch: TodoUpdateInput): Observable<Todo>;
  /** Deletes a todo, refreshes the list automatically on success */
  remove(id: string): Observable<void>;
}
