import { greet } from './esm-chunk.js';

export function render(data) {
  const parsed = typeof data === 'string' ? JSON.parse(data) : data;
  const name = (parsed && parsed.name) || 'admin';
  return '<div class="admin">' + greet(name) + '</div>';
}
