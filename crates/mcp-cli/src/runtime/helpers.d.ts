export interface PageResult<T> {
  items: T[];
  nextCursor?: string | null;
}

export function paginate<T>(
  fetchPage: (cursor: string | undefined) => Promise<PageResult<T>>,
  options?: { maxPages?: number; maxItems?: number },
): Promise<{ items: T[]; truncated: boolean }>;

export function mapBounded<T, R>(
  input: Iterable<T>,
  fn: (item: T, index: number) => Promise<R> | R,
  options?: { concurrency?: number; maxItems?: number },
): Promise<{ results: R[]; truncated: boolean }>;
