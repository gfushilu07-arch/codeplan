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
  const token = getToken();
  const headers = new Headers(init?.headers);
  if (token) headers.set("Authorization", `Bearer ${token}`);
  const res = await fetch(`${baseUrl}${path}`, { ...init, headers });
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

/** 登录响应。 */
export interface SessionInfo {
  requires_totp: boolean;
  token?: string;
  expires_at?: string;
  ticket?: string;
  user_id: string;
  display_name: string;
  is_platform_admin: boolean;
}

/** POST /api/auth/login */
export async function login(
  username: string,
  password: string,
  baseUrl = "",
): Promise<SessionInfo> {
  const res = await fetch(`${baseUrl}/api/auth/login`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ username, password }),
  });
  if (!res.ok) {
    const errBody = (await res.json().catch(() => null)) as { message?: string } | null;
    throw new ApiError("ORG-401", errBody?.message ?? res.statusText, res.status);
  }
  const body = (await res.json()) as { data: SessionInfo };
  return body.data;
}

/** GET /api/me */
export async function fetchMe(baseUrl = ""): Promise<MeInfo> {
  return request<MeInfo>("/api/me", baseUrl, {
    headers: { Authorization: `Bearer ${getToken()}` },
  });
}

export interface MeInfo {
  id: string;
  display_name: string;
  email: string;
  is_platform_admin: boolean;
}

/** 会话令牌存取。 */
export function getToken(): string | null {
  return typeof localStorage !== "undefined" ? localStorage.getItem("cp_token") : null;
}

export function setToken(token: string | null): void {
  if (token) localStorage.setItem("cp_token", token);
  else localStorage.removeItem("cp_token");
}

/** GET /api/dashboard/my-items */
export interface MyItem {
  id: string;
  item_type: string;
  title: string;
  status: string;
  project_id: string;
  updated_at: string;
}

export async function fetchMyItems(baseUrl = ""): Promise<MyItem[]> {
  return request<MyItem[]>("/api/dashboard/my-items", baseUrl, {
    headers: { Authorization: `Bearer ${getToken() ?? ""}` },
  });
}
