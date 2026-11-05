/**
 * Service assembly entry: mock/real switching happens only here.
 * Components always import todoService from '../services'; importing a
 * concrete implementation directly is forbidden.
 * Component mode needs no login (the shell-injected token is carried
 * uniformly by apiClient); no auth service.
 */
import { realTodoService } from './todoService';
import { mockTodoService } from './mock/mockTodoService';
import type { TodoService } from './interfaces';

const useMock = import.meta.env.VITE_USE_MOCK === 'true';

export const todoService: TodoService = useMock ? mockTodoService : realTodoService;
