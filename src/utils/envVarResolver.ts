/**
 * Extract all {{variable}} placeholder names from a template string.
 * Returns unique variable names in order of first appearance.
 */
export function extractVariables(template: string): string[] {
  const regex = /\{\{(\w+)\}\}/g
  const names: string[] = []
  const seen = new Set<string>()
  let match: RegExpExecArray | null
  while ((match = regex.exec(template)) !== null) {
    if (!seen.has(match[1])) {
      seen.add(match[1])
      names.push(match[1])
    }
  }
  return names
}

/**
 * Replace {{variable}} placeholders with values from bindings.
 * Unresolved placeholders are left as-is.
 */
export function resolveTemplate(
  template: string,
  bindings: Record<string, string>,
): string {
  return template.replace(/\{\{(\w+)\}\}/g, (_, name: string) => {
    return bindings[name] ?? `{{${name}}}`
  })
}

/**
 * Extract user property placeholder names from a list of key-value pairs.
 * Both key and value can contain {{variables}}.
 */
export function extractUserPropertyVariables(
  userProperties: { key: string; value: string }[],
): string[] {
  const names = new Set<string>()
  for (const prop of userProperties) {
    for (const p of extractVariables(prop.key)) names.add(p)
    for (const p of extractVariables(prop.value)) names.add(p)
  }
  return [...names]
}

/**
 * Resolve user property key-value pairs with variable bindings.
 */
export function resolveUserProperties(
  userProperties: { key: string; value: string }[],
  bindings: Record<string, string>,
): { key: string; value: string }[] {
  return userProperties.map((p) => ({
    key: resolveTemplate(p.key, bindings),
    value: resolveTemplate(p.value, bindings),
  }))
}