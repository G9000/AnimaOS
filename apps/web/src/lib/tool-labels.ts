/** A tool's name as a person would say it: `search_tools` → "Search tools".
 *  The raw name stays available as a tooltip where it is shown. */
export function toolLabel(name: string): string {
  if (/^(remember|memory_(?:store|write|remember))/i.test(name))
    return 'Remembered';
  if (/^mcp_[a-f0-9]{16,}(?:_[a-f0-9]{12,})?$/i.test(name))
    return 'Connected tool';
  const words = name.replace(/[_-]+/g, ' ').trim();
  return words ? words[0].toUpperCase() + words.slice(1) : 'Completed action';
}
