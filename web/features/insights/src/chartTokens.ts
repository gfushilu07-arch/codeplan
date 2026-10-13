/** 读 design-tokens 的 CSS 变量,供图表配色复用(独立于 Chart,避免把 echarts 带进主包)。 */
export function token(name: string, fallback: string): string {
  if (typeof window === "undefined") return fallback;
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return v || fallback;
}
