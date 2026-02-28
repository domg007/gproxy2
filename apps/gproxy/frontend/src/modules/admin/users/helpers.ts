export function maskApiKey(value: string): string {
  const key = value.trim();
  if (!key) {
    return "";
  }
  if (key.length <= 8) {
    return "****";
  }
  const prefix = key.slice(0, 4);
  const suffix = key.slice(-4);
  const mask = "*".repeat(Math.max(4, key.length - 8));
  return `${prefix}${mask}${suffix}`;
}
