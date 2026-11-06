/**
 * A single todo: toggle completion, inline editing (title/notes/due time),
 * delete (double-confirm against accidental taps).
 * Updates use optimistic updates (the service layer rolls back and refreshes
 * on failure); the list is refreshed by the service layer after deletion.
 */
import { useEffect, useRef, useState } from 'react';
import { todoService } from '../services';
import { formatDue, isOverdue, isoToLocalInput, localInputToIso } from '../utils/datetime';
import type { Todo, TodoUpdateInput } from '../types';

interface Props {
  todo: Todo;
}

export function TodoItem({ todo }: Props) {
  const [editing, setEditing] = useState(false);
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  // Edit drafts (copied from the current todo when entering edit mode)
  const [draftTitle, setDraftTitle] = useState('');
  const [draftNotes, setDraftNotes] = useState('');
  const [draftDue, setDraftDue] = useState('');

  // 3-second auto-reset timer for the delete double-confirm
  const confirmTimerRef = useRef<number | null>(null);
  useEffect(
    () => () => {
      if (confirmTimerRef.current !== null) window.clearTimeout(confirmTimerRef.current);
    },
    [],
  );

  const startEdit = () => {
    setDraftTitle(todo.title);
    setDraftNotes(todo.notes ?? '');
    setDraftDue(isoToLocalInput(todo.due_at));
    setActionError(null);
    setEditing(true);
  };

  const runUpdate = (patch: TodoUpdateInput, onDone?: () => void) => {
    setSaving(true);
    setActionError(null);
    todoService.update(todo.id, patch).subscribe({
      next: () => {
        setSaving(false);
        onDone?.();
      },
      error: (err: Error) => {
        // On failure the service layer has already rolled the list back; here
        // we only surface an inline notice
        setActionError(err.message || '保存失败');
        setSaving(false);
      },
    });
  };

  const saveEdit = () => {
    const trimmedTitle = draftTitle.trim();
    if (!trimmedTitle) {
      setActionError('标题不能为空');
      return;
    }
    runUpdate(
      {
        title: trimmedTitle,
        notes: draftNotes.trim(), // an empty string clears the notes (contract: notes: string)
        due_at: localInputToIso(draftDue), // null clears the due time (allowed by the contract)
      },
      () => setEditing(false),
    );
  };

  const handleDelete = () => {
    if (confirmTimerRef.current !== null) {
      window.clearTimeout(confirmTimerRef.current);
      confirmTimerRef.current = null;
    }
    if (!confirmingDelete) {
      setConfirmingDelete(true);
      confirmTimerRef.current = window.setTimeout(() => setConfirmingDelete(false), 3000);
      return;
    }
    setDeleting(true);
    todoService.remove(todo.id).subscribe({
      error: (err: Error) => {
        setDeleting(false);
        setConfirmingDelete(false);
        setActionError(err.message || '删除失败');
      },
    });
  };

  if (editing) {
    return (
      <li className="todo-item card todo-item--editing">
        <input
          className="input"
          value={draftTitle}
          autoFocus
          aria-label="标题"
          onChange={(e) => setDraftTitle(e.target.value)}
        />
        <textarea
          className="input"
          rows={2}
          placeholder="备注（可选）"
          aria-label="备注"
          value={draftNotes}
          onChange={(e) => setDraftNotes(e.target.value)}
        />
        <input
          className="input"
          type="datetime-local"
          aria-label="截止时间"
          value={draftDue}
          onChange={(e) => setDraftDue(e.target.value)}
        />
        {actionError && (
          <p className="form-error" role="alert">
            {actionError}
          </p>
        )}
        <div className="todo-item-actions">
          <button type="button" className="btn btn-primary" onClick={saveEdit} disabled={saving}>
            {saving ? '保存中…' : '保存'}
          </button>
          <button
            type="button"
            className="btn btn-ghost"
            onClick={() => setEditing(false)}
            disabled={saving}
          >
            取消
          </button>
        </div>
      </li>
    );
  }

  const overdue = !todo.completed && todo.due_at !== null && isOverdue(todo.due_at);

  return (
    <li className={`todo-item card${todo.completed ? ' is-completed' : ''}`}>
      <label className="todo-item-main">
        <input
          type="checkbox"
          className="todo-check"
          checked={todo.completed}
          onChange={() => runUpdate({ completed: !todo.completed })}
        />
        <span className="todo-title">{todo.title}</span>
      </label>
      {todo.notes && <p className="todo-notes">{todo.notes}</p>}
      {todo.due_at && (
        <p className={`todo-due${overdue ? ' is-overdue' : ''}`}>
          ⏰ {formatDue(todo.due_at)}
          {overdue ? '（已逾期）' : ''}
        </p>
      )}
      {actionError && (
        <p className="form-error" role="alert">
          {actionError}
        </p>
      )}
      <div className="todo-item-actions">
        <button
          type="button"
          className="btn btn-ghost"
          onClick={startEdit}
          disabled={saving || deleting}
        >
          编辑
        </button>
        <button
          type="button"
          className={`btn ${confirmingDelete ? 'btn-danger' : 'btn-ghost'}`}
          onClick={handleDelete}
          disabled={saving || deleting}
        >
          {deleting ? '删除中…' : confirmingDelete ? '确认删除？' : '删除'}
        </button>
      </div>
    </li>
  );
}
