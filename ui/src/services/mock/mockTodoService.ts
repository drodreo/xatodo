/**
 * Mock todo service: in-memory array + simulated network latency,
 * implementing exactly the same TodoService interface as the real service.
 * Purpose: supports UI debugging when VITE_USE_MOCK=true (server not ready).
 * Behavior aligned with the real implementation: refresh follows
 * server-side filter semantics, update takes effect optimistically,
 * commands carry latency.
 */
import {
  BehaviorSubject,
  combineLatest,
  distinctUntilChanged,
  map,
  mergeMap,
  switchMap,
  tap,
  throwError,
  timer,
} from 'rxjs';
import type { Observable } from 'rxjs';
import type { TodoService } from '../interfaces';
import type {
  ListError,
  ListStatus,
  Todo,
  TodoCreateInput,
  TodoFilter,
  TodoUpdateInput,
} from '../../types';

const MOCK_USER_ID = '00000000-0000-0000-0000-000000000001';
const LATENCY_MS = 250;

/** Generates an ISO timestamp relative to now (offsetHours may be negative) */
function iso(offsetHours = 0): string {
  return new Date(Date.now() + offsetHours * 3600_000).toISOString();
}

/** Generates an id: crypto.randomUUID preferred (fallback for non-secure
 * contexts) */
function newId(): string {
  return typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
    ? crypto.randomUUID()
    : `${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function seedTodos(): Todo[] {
  return [
    {
      id: newId(),
      user_id: MOCK_USER_ID,
      title: '熟悉 XaTodo 的基本操作',
      notes: '勾选完成、行内编辑、筛选、删除都可以试一遍',
      completed: false,
      due_at: iso(24),
      created_at: iso(-48),
      updated_at: iso(-48),
    },
    {
      id: newId(),
      user_id: MOCK_USER_ID,
      title: '检查 server 侧 API 契约实现进度',
      notes: null,
      completed: false,
      due_at: iso(72),
      created_at: iso(-24),
      updated_at: iso(-24),
    },
    {
      id: newId(),
      user_id: MOCK_USER_ID,
      title: '阅读技术文档',
      notes: '重点整理核心章节的要点',
      completed: true,
      due_at: null,
      created_at: iso(-96),
      updated_at: iso(-72),
    },
  ];
}

class MockTodoService implements TodoService {
  private items: Todo[] = seedTodos();

  readonly todos$ = new BehaviorSubject<Todo[]>([]);
  readonly listStatus$ = new BehaviorSubject<ListStatus>('idle');
  readonly listError$ = new BehaviorSubject<ListError | null>(null);
  readonly filter$ = new BehaviorSubject<TodoFilter>('all');

  readonly filteredTodos$ = combineLatest([this.todos$, this.filter$]).pipe(
    map(([todos, filter]) => this.applyFilter(todos, filter)),
  );

  constructor() {
    this.filter$.pipe(distinctUntilChanged(), switchMap(() => this.refresh())).subscribe();
  }

  setFilter(filter: TodoFilter): void {
    this.filter$.next(filter);
  }

  refresh(): Observable<void> {
    this.listStatus$.next('loading');
    this.listError$.next(null);
    return timer(LATENCY_MS).pipe(
      map(() => this.applyFilter(this.items, this.filter$.getValue())),
      tap((todos) => {
        this.todos$.next(todos);
        this.listStatus$.next('ready');
      }),
      map(() => undefined as void),
    );
  }

  create(input: TodoCreateInput): Observable<Todo> {
    return timer(LATENCY_MS).pipe(
      map(() => {
        const now = new Date().toISOString();
        const todo: Todo = {
          id: newId(),
          user_id: MOCK_USER_ID,
          title: input.title,
          notes: input.notes ?? null,
          completed: false,
          due_at: input.due_at ?? null,
          created_at: now,
          updated_at: now,
        };
        this.items = [todo, ...this.items];
        return todo;
      }),
      tap(() => this.refresh().subscribe()),
    );
  }

  update(id: string, patch: TodoUpdateInput): Observable<Todo> {
    const idx = this.items.findIndex((t) => t.id === id);
    if (idx < 0) {
      return timer(LATENCY_MS).pipe(
        mergeMap(() => throwError(() => new Error('待办不存在'))),
      );
    }
    // Simulated optimistic update: takes effect locally immediately, returns
    // the "server-confirmed value" after the delay
    const updated: Todo = {
      ...this.items[idx],
      ...(patch.title !== undefined && { title: patch.title }),
      ...(patch.notes !== undefined && { notes: patch.notes }),
      ...(patch.completed !== undefined && { completed: patch.completed }),
      ...(patch.due_at !== undefined && { due_at: patch.due_at }),
      updated_at: new Date().toISOString(),
    };
    this.items = this.items.map((t, i) => (i === idx ? updated : t));
    this.pushFiltered();
    return timer(LATENCY_MS).pipe(map(() => updated));
  }

  remove(id: string): Observable<void> {
    return timer(LATENCY_MS).pipe(
      map(() => {
        this.items = this.items.filter((t) => t.id !== id);
        return undefined as void;
      }),
      tap(() => this.refresh().subscribe()),
    );
  }

  private pushFiltered(): void {
    this.todos$.next(this.applyFilter(this.items, this.filter$.getValue()));
  }

  private applyFilter(todos: Todo[], filter: TodoFilter): Todo[] {
    if (filter === 'all') return todos;
    return todos.filter((t) => (filter === 'completed' ? t.completed : !t.completed));
  }
}

export const mockTodoService = new MockTodoService();
