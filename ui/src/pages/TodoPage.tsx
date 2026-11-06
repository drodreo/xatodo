/**
 * Todo home page: create, filter, list display.
 * All data comes from todoService's Observable streams (subscribed via
 * useObservable); this component issues no requests directly.
 * Intermediate states: loading skeleton / error retry / empty state.
 */
import { useEffect } from 'react';
import { todoService } from '../services';
import { useObservable } from '../hooks/useObservable';
import { TodoComposer } from '../components/TodoComposer';
import { FilterTabs } from '../components/FilterTabs';
import { TodoItem } from '../components/TodoItem';
import type { TodoFilter } from '../types';

const FILTERS: { key: TodoFilter; label: string }[] = [
  { key: 'all', label: '全部' },
  { key: 'active', label: '未完成' },
  { key: 'completed', label: '已完成' },
];

export function TodoPage() {
  const todos = useObservable(todoService.filteredTodos$, []);
  const listStatus = useObservable(todoService.listStatus$, 'idle');
  const listError = useObservable(todoService.listError$, null);
  const filter = useObservable(todoService.filter$, 'all');

  // Fetch the list on first entry (later refreshes are triggered internally
  // by service-layer commands)
  useEffect(() => {
    const sub = todoService.refresh().subscribe();
    return () => sub.unsubscribe();
  }, []);

  return (
    <div className="todo-page">
      <TodoComposer />
      <FilterTabs filters={FILTERS} value={filter} onChange={(f) => todoService.setFilter(f)} />

      {listStatus === 'error' && listError && (
        <div className="todo-error" role="alert">
          <span className="todo-error-message">{listError.message}</span>
          <div className="todo-error-actions">
            <button
              type="button"
              className="btn btn-ghost"
              onClick={() => todoService.refresh().subscribe()}
            >
              重试
            </button>
          </div>
        </div>
      )}

      {listStatus === 'loading' && (
        <div className="todo-skeleton" aria-hidden="true">
          <div className="skeleton-card" />
          <div className="skeleton-card" />
          <div className="skeleton-card" />
        </div>
      )}

      {listStatus === 'ready' && todos.length === 0 && (
        <div className="todo-empty">
          <p className="todo-empty-title">这里空空如也</p>
          <p className="todo-empty-sub">在上方输入第一件待办吧</p>
        </div>
      )}

      {listStatus === 'ready' && todos.length > 0 && (
        <ul className="todo-list">
          {todos.map((todo) => (
            <TodoItem key={todo.id} todo={todo} />
          ))}
        </ul>
      )}
    </div>
  );
}
