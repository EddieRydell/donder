/** `count` of `noun`, in the singular for one: "1 universe", "3 universes". */
export function countOf(count: number, noun: string, plural = `${noun}s`): string {
  return `${count} ${count === 1 ? noun : plural}`;
}
