/**
 * Todo service (real implementation): remote inputs are uniformly modeled
 * as Observables.
 * todos$ is the single authoritative list state; command methods refresh the
 * list on success; update uses optimistic updates + rollback on failure to
 * keep interaction feedback immediate.
 * Component mode needs no login (the shell-injected token is carried
 * uniformly by apiClient); no credential short-circuit logic.
 * Request paths are the component's own full contract /api/v1/... (the shell
 * strips the /api/todo namespace prefix).
 */
import {
  EMPTY,
  BehaviorSubject,
  catchError,
  combineLatest,
  defer,
  distinctUntilChanged,
  from,
  map,
  switchMap,
  tap,
  throwError,
} from 'rxjs';
import type { Observable } from 'rxjs';
import { ApiError, apiFetch } from './apiClient';
import type { TodoService } from './interfaces';
import type {
  ListError,
  ListStatus,
  Todo,
  TodoCreateInput,
  TodoFilter,
  TodoUpdateInput,
} from '../types';

/** Server-side filter params (contract: GET /api/v1/todos?completed=true|false) */
const FILTER_QUERY: Record<Exclude<TodoFilter, 'all'>, 'false' | 'true'> = {
  active: 'false',
  completed: 'true',
};

class RealTodoService implements TodoService {
  readonly todos$ = new BehaviorSubject<Todo[]>([]);
  readonly listStatus$ = new BehaviorSubject<ListStatus>('idle');
  readonly listError$ = new BehaviorSubject<ListError | null>(null);
  readonly filter$ = new BehaviorSubject<TodoFilter>('all');

  /** Display stream: the client re-filters by filter (idempotent safety net,
   * compatible with the optimistic-update window) */
  readonly filteredTodos$ = combineLatest([this.todos$, this.filter$]).pipe(
    map(([todos, filter]) =>
      filter === 'all'
        ? todos
        : todos.filter((t) => (filter === 'completed' ? t.completed : !t.completed)),
    ),
  );

  constructor() {
    // Filter change → automatic re-fetch (server-side filtering is primary)
    this.filter$.pipe(distinctUntilChanged(), switchMap(() => this.refresh())).subscribe();
  }

  setFilter(filter: TodoFilter): void {
    this.filter$.next(filter);
  }

  refresh(): Observable<void> {
    return defer(() => {
      // defer wrapper: reads the latest filter on every subscription and
      // enters loading
      const filter = this.filter$.getValue();
      const query = filter === 'all' ? undefined : { completed: FILTER_QUERY[filter] };
      this.listStatus$.next('loading');
      this.listError$.next(null);
      return from(apiFetch<Todo[]>('/api/v1/todos', { query })).pipe(
        tap((todos) => {
          this.todos$.next(todos);
          this.listStatus$.next('ready');
        }),
        map(() => undefined as void),
        catchError((err: unknown) => {
          this.listStatus$.next('error');
          this.listError$.next(toListError(err));
          return EMPTY; // the error has been funneled into the list error
          // stream; subscribers need not be aware
        }),
      );
    });
  }

  create(input: TodoCreateInput): Observable<Todo> {
    return from(apiFetch<Todo>('/api/v1/todos', { method: 'POST', body: input })).pipe(
      tap(() => this.refresh().subscribe()),
    );
  }

  update(id: string, patch: TodoUpdateInput): Observable<Todo> {
    // Optimistic update: apply the change to the local stream first, roll
    // back wholesale on failure
    const snapshot = this.todos$.getValue();
    this.todos$.next(snapshot.map((t) => (t.id === id ? { ...t, ...pickDefined(patch) } : t)));
    return from(apiFetch<Todo>(`/api/v1/todos/${id}`, { method: 'PATCH', body: patch })).pipe(
      tap((updated) => {
        // Replace the local optimistic value with the server's authoritative
        // value
        this.todos$.next(this.todos$.getValue().map((t) => (t.id === id ? updated : t)));
      }),
      catchError((err: unknown) => {
        this.todos$.next(snapshot); // roll back to the pre-update snapshot
        this.refresh().subscribe();
        return throwError(() => err);
      }),
    );
  }

  remove(id: string): Observable<void> {
    return from(apiFetch<void>(`/api/v1/todos/${id}`, { method: 'DELETE' })).pipe(
      map(() => undefined as void),
      tap(() => this.refresh().subscribe()),
    );
  }
}

/** Only fields explicitly present in the patch participate in the optimistic
 * merge (fields not passed keep their local state) */
function pickDefined(patch: TodoUpdateInput): Partial<Todo> {
  const result: Partial<Todo> = {};
  if (patch.title !== undefined) result.title = patch.title;
  if (patch.notes !== undefined) result.notes = patch.notes;
  if (patch.completed !== undefined) result.completed = patch.completed;
  if (patch.due_at !== undefined) result.due_at = patch.due_at;
  return result;
}

function toListError(err: unknown): ListError {
  if (err instanceof ApiError) {
    return { message: err.message, unauthorized: err.status === 401 };
  }
  return { message: err instanceof Error ? err.message : '加载失败', unauthorized: false };
}

export const realTodoService = new RealTodoService();
