/**
 * Filter tab group: all / active / completed.
 */
import type { TodoFilter } from '../types';

interface FilterOption {
  key: TodoFilter;
  label: string;
}

interface Props {
  filters: FilterOption[];
  value: TodoFilter;
  onChange: (filter: TodoFilter) => void;
}

export function FilterTabs({ filters, value, onChange }: Props) {
  return (
    <div className="filter-tabs" role="tablist" aria-label="待办筛选">
      {filters.map((f) => (
        <button
          key={f.key}
          type="button"
          role="tab"
          aria-selected={f.key === value}
          className={`filter-tab${f.key === value ? ' is-active' : ''}`}
          onClick={() => onChange(f.key)}
        >
          {f.label}
        </button>
      ))}
    </div>
  );
}
