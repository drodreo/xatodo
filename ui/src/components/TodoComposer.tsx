/**
 * Create form: a title input + expandable notes / due_at advanced fields.
 * Submission goes through todoService.create(); the form is cleared on
 * success (the list is refreshed by the service layer).
 */
import { useState, type FormEvent } from 'react';
import { todoService } from '../services';
import { localInputToIso } from '../utils/datetime';

export function TodoComposer() {
  const [title, setTitle] = useState('');
  const [expanded, setExpanded] = useState(false);
  const [notes, setNotes] = useState('');
  const [dueLocal, setDueLocal] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const canSubmit = title.trim().length > 0 && !submitting;

  const handleSubmit = (e: FormEvent) => {
    e.preventDefault();
    if (!canSubmit) return;
    const trimmedTitle = title.trim();
    const trimmedNotes = notes.trim();
    const dueIso = localInputToIso(dueLocal);
    setError(null);
    setSubmitting(true);
    todoService
      .create({
        title: trimmedTitle,
        // Empty-value fields are omitted per the contract (the backend
        // stores null)
        ...(trimmedNotes ? { notes: trimmedNotes } : {}),
        ...(dueIso ? { due_at: dueIso } : {}),
      })
      .subscribe({
        next: () => {
          setTitle('');
          setNotes('');
          setDueLocal('');
          setExpanded(false);
          setSubmitting(false);
        },
        error: (err: Error) => {
          setError(err.message || '创建失败，请重试');
          setSubmitting(false);
        },
      });
  };

  return (
    <form className="composer card" onSubmit={handleSubmit}>
      <div className="composer-row">
        <input
          className="input composer-title"
          placeholder="要做点什么？"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
        />
        <button className="btn btn-primary" type="submit" disabled={!canSubmit}>
          {submitting ? '添加中…' : '添加'}
        </button>
        <button
          className="btn btn-ghost composer-toggle"
          type="button"
          aria-expanded={expanded}
          onClick={() => setExpanded((v) => !v)}
        >
          {expanded ? '收起' : '详情'}
        </button>
      </div>
      {expanded && (
        <div className="composer-extra">
          <textarea
            className="input composer-notes"
            placeholder="备注（可选）"
            rows={2}
            value={notes}
            onChange={(e) => setNotes(e.target.value)}
          />
          <label className="composer-due">
            <span className="composer-due-label">截止时间</span>
            <input
              className="input"
              type="datetime-local"
              value={dueLocal}
              onChange={(e) => setDueLocal(e.target.value)}
            />
          </label>
        </div>
      )}
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
    </form>
  );
}
