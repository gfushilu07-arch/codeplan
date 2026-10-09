/** 设计令牌包:CSS 变量见 tokens.css;TS 侧仅提供令牌元数据。 */
export const DESIGN_TOKENS_THEME_ATTR = "data-theme" as const;

/** 切换亮/暗主题(暗色为一级公民,见 ARCHITECTURE.md §2.3)。 */
export function setTheme(theme: "light" | "dark"): void {
  document.documentElement.setAttribute(DESIGN_TOKENS_THEME_ATTR, theme);
}
