/** 通用工具(前端版 common:无业务语义,见 web/AGENTS.md 公共代码决策树)。 */

/** 拼接 class 名,过滤假值。 */
export function cn(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}

/** 后端错误码格式:<域>-<3位>(如 GIT-404),见 ARCHITECTURE.md §1.3。 */
export const ERROR_CODE_PATTERN = /^[A-Z]+-\d{3}$/;

/** 判断字符串是否为合法平台错误码。 */
export function isErrorCode(value: string): boolean {
  return ERROR_CODE_PATTERN.test(value);
}
