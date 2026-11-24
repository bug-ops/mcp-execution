const DEFAULT_MAX_PAGES = 20;
const DEFAULT_MAX_ITEMS = 1000;
const DEFAULT_CONCURRENCY = 4;
const MAX_CONCURRENCY = 16;

/**
 * Collects the items of a paginated call, stopping at `maxPages` pages or `maxItems` items.
 * `fetchPage(cursor)` resolves `{ items, nextCursor }`; `truncated` tells whether items remain.
 */
export async function paginate(fetchPage, options = {}) {
  const maxPages = options.maxPages ?? DEFAULT_MAX_PAGES;
  const maxItems = options.maxItems ?? DEFAULT_MAX_ITEMS;
  const items = [];
  let cursor;
  for (let page = 0; page < maxPages; page += 1) {
    const result = await fetchPage(cursor);
    items.push(...result.items);
    cursor = result.nextCursor;
    if (items.length > maxItems) {
      items.length = maxItems;
      return { items, truncated: true };
    }
    if (cursor === undefined || cursor === null) {
      return { items, truncated: false };
    }
  }
  return { items, truncated: true };
}

/**
 * Maps `fn(item, index)` over `input` with at most `concurrency` calls in flight, processing at
 * most `maxItems` items; `truncated` tells whether input items were left out. Once a call
 * rejects no new item is started and the returned promise rejects.
 */
export async function mapBounded(input, fn, options = {}) {
  const concurrency = Math.max(1, Math.min(options.concurrency ?? DEFAULT_CONCURRENCY, MAX_CONCURRENCY));
  const maxItems = options.maxItems ?? DEFAULT_MAX_ITEMS;
  const list = [...input];
  const truncated = list.length > maxItems;
  const work = truncated ? list.slice(0, maxItems) : list;
  const results = new Array(work.length);
  let next = 0;
  let failed = false;
  const worker = async () => {
    while (!failed && next < work.length) {
      const index = next;
      next += 1;
      try {
        results[index] = await fn(work[index], index);
      } catch (error) {
        // Stop pulling new items; calls already in flight finish on their own.
        failed = true;
        throw error;
      }
    }
  };
  await Promise.all(Array.from({ length: Math.min(concurrency, work.length) }, worker));
  return { results, truncated };
}
