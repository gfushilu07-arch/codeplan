/**
 * 类型化 API 客户端:`generated.ts` 为生成物,本文件是手写的薄封装(请求/错误处理)。
 * 新端点先在后端加 utoipa 注解,再运行 `pnpm gen:api` 重新生成。
 */
import type { components } from "./generated";

export type Schemas = components["schemas"];
export type Health = Schemas["Health"];

/** 后端 API 错误:携带平台错误码(<域>-<3位>,见 ARCHITECTURE.md §1.3)。 */
export class ApiError extends Error {
  readonly code: string;
  readonly status: number;

  constructor(code: string, message: string, status: number) {
    super(message);
    this.name = "ApiError";
    this.code = code;
    this.status = status;
  }
}

async function request<T>(path: string, baseUrl: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${baseUrl}${path}`, init);
  if (!res.ok) {
    const body = (await res.json().catch(() => null)) as {
      code?: string;
      message?: string;
    } | null;
    throw new ApiError(body?.code ?? "UNK-500", body?.message ?? res.statusText, res.status);
  }
  return (await res.json()) as T;
}

/** GET /api/health */
export function fetchHealth(baseUrl = ""): Promise<Health> {
  return request<Health>("/api/health", baseUrl);
}
