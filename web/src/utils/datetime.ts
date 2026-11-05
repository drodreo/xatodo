/** Datetime utilities: conversions between ISO 8601 and datetime-local
 * control values, plus display formatting */

/** ISO → datetime-local control value (local timezone, minute precision);
 * returns an empty string for null/invalid input */
export function isoToLocalInput(iso: string | null | undefined): string {
  if (!iso) return '';
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '';
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** datetime-local control value → ISO 8601; returns null for empty/invalid
 * input (matches the contract where due_at can be null) */
export function localInputToIso(value: string): string | null {
  if (!value) return null;
  const d = new Date(value);
  return Number.isNaN(d.getTime()) ? null : d.toISOString();
}

/** Display formatting (local timezone) */
export function formatDue(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString('zh-CN', {
    month: 'numeric',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

/** Whether the todo is overdue (whether to render it in red is decided by
 * the caller combined with completed) */
export function isOverdue(iso: string): boolean {
  const t = new Date(iso).getTime();
  return !Number.isNaN(t) && t < Date.now();
}
