/** Small formatting helpers shared across views. */

/** `count(1, "task")` -> "1 task"; `count(3, "task")` -> "3 tasks". */
export function count(value: number, singular: string, plural?: string): string {
  const word = value === 1 ? singular : (plural ?? `${singular}s`);
  return `${value} ${word}`;
}
