/**
 * Pretty-print a JSON string if it's valid JSON.
 * Returns the original string if it's not valid JSON.
 */
export function prettyJson(str: string): string {
  if (!str || str.trim() === '') return str
  try {
    const parsed = JSON.parse(str)
    return JSON.stringify(parsed, null, 2)
  } catch {
    return str
  }
}

/**
 * Check if a string is valid JSON.
 */
export function isJson(str: string): boolean {
  try {
    JSON.parse(str)
    return true
  } catch {
    return false
  }
}

/**
 * Truncate a string to a maximum length, adding an ellipsis if truncated.
 */
export function truncate(str: string, maxLen: number): string {
  if (str.length <= maxLen) return str
  return str.substring(0, maxLen) + '...'
}