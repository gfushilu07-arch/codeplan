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


// ---------- 看板(T-113/T-132) ----------

/** 看板条目。 */
export interface BoardItem {
  id: string;
  number: number;
  title: string;
  priority: string;
  board_order: number;
}

/** 看板列。 */
export interface BoardColumn {
  status: string;
  total: number;
  items: BoardItem[];
}

/** 看板视图。 */
export interface BoardView {
  item_type: string;
  columns: BoardColumn[];
}

/** GET /api/projects/{key}/board */
export async function fetchBoard(
  projectKey: string,
  itemType = "story",
  baseUrl = "",
): Promise<BoardView> {
  const res = await fetch(
    `${baseUrl}/api/projects/${projectKey}/board?item_type=${itemType}`,
    { headers: { Authorization: `Bearer ${getToken() ?? ""}` } },
  );
  if (!res.ok) throw new ApiError("PM-500", res.statusText, res.status);
  const body = (await res.json()) as { data: BoardView };
  return body.data;
}

/** PUT /api/projects/{key}/board/reorder */
export async function reorderBoard(
  projectKey: string,
  items: Array<{ id: string; status: string; board_order: number }>,
  baseUrl = "",
): Promise<void> {
  const res = await fetch(`${baseUrl}/api/projects/${projectKey}/board/reorder`, {
    method: "PUT",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${getToken() ?? ""}` },
    body: JSON.stringify({ items }),
  });
  if (!res.ok) throw new ApiError("PM-500", res.statusText, res.status);
}

/** 项目列表条目。 */
export interface ProjectSummary {
  id: string;
  key: string;
  name: string;
  description: string;
  created_at: string;
}

/** GET /api/projects */
export async function listProjects(baseUrl = ""): Promise<ProjectSummary[]> {
  const res = await fetch(`${baseUrl}/api/projects`, {
    headers: { Authorization: `Bearer ${getToken() ?? ""}` },
  });
  if (!res.ok) throw new ApiError("PM-500", res.statusText, res.status);
  const body = (await res.json()) as { data: ProjectSummary[] };
  return body.data;
}


// ---------- Wiki(T-115/T-133) ----------

/** Wiki 页面(源 + 渲染 HTML)。 */
export interface WikiPage {
  id: string;
  slug: string;
  title: string;
  content: string;
  html: string;
  parent_id: string | null;
  updated_at: string;
}

/** 页面树节点。 */
export interface WikiTreeNode {
  id: string;
  slug: string;
  title: string;
  parent_id: string | null;
  updated_at: string;
  children: WikiTreeNode[];
}

/** GET /api/projects/{id}/wiki */
export async function wikiTree(projectId: string, baseUrl = ""): Promise<WikiTreeNode[]> {
  const res = await fetch(`${baseUrl}/api/projects/${projectId}/wiki`, {
    headers: { Authorization: `Bearer ${getToken() ?? ""}` },
  });
  if (!res.ok) throw new ApiError("WIKI-500", res.statusText, res.status);
  const body = (await res.json()) as { data: WikiTreeNode[] };
  return body.data;
}

/** GET /api/projects/{id}/wiki/{slug} */
export async function fetchWikiPage(
  projectId: string,
  slug: string,
  baseUrl = "",
): Promise<WikiPage> {
  const res = await fetch(`${baseUrl}/api/projects/${projectId}/wiki/${slug}`, {
    headers: { Authorization: `Bearer ${getToken() ?? ""}` },
  });
  if (!res.ok) throw new ApiError("WIKI-404", res.statusText, res.status);
  const body = (await res.json()) as { data: WikiPage };
  return body.data;
}

/** POST /api/projects/{id}/wiki */
export async function createWikiPage(
  projectId: string,
  body: { slug: string; title: string; content: string; parent_id?: string },
  baseUrl = "",
): Promise<WikiPage> {
  const res = await fetch(`${baseUrl}/api/projects/${projectId}/wiki`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${getToken() ?? ""}` },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const err = (await res.json().catch(() => null)) as { message?: string } | null;
    throw new ApiError("WIKI-400", err?.message ?? res.statusText, res.status);
  }
  const data = (await res.json()) as { data: WikiPage };
  return data.data;
}

/** PUT /api/projects/{id}/wiki/{slug} */
export async function updateWikiPage(
  projectId: string,
  slug: string,
  body: { title: string; content: string },
  baseUrl = "",
): Promise<WikiPage> {
  const res = await fetch(`${baseUrl}/api/projects/${projectId}/wiki/${slug}`, {
    method: "PUT",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${getToken() ?? ""}` },
    body: JSON.stringify(body),
  });
  if (!res.ok) throw new ApiError("WIKI-500", res.statusText, res.status);
  const data = (await res.json()) as { data: WikiPage };
  return data.data;
}

/** DELETE /api/projects/{id}/wiki/{slug} */
export async function deleteWikiPage(projectId: string, slug: string, baseUrl = ""): Promise<void> {
  const res = await fetch(`${baseUrl}/api/projects/${projectId}/wiki/${slug}`, {
    method: "DELETE",
    headers: { Authorization: `Bearer ${getToken() ?? ""}` },
  });
  if (!res.ok) throw new ApiError("WIKI-500", res.statusText, res.status);
}

/** 上传附件。返回附件 ID。 */
export async function uploadAttachment(
  projectId: string,
  slug: string,
  filename: string,
  bytes: Blob | Uint8Array,
  baseUrl = "",
): Promise<{ id: string; filename: string; size_bytes: number }> {
  const res = await fetch(
    `${baseUrl}/api/projects/${projectId}/wiki/${slug}/attachments?filename=${encodeURIComponent(filename)}`,
    { method: "POST", headers: { Authorization: `Bearer ${getToken() ?? ""}` }, body: bytes as Blob },
  );
  if (!res.ok) throw new ApiError("WIKI-400", res.statusText, res.status);
  const body = (await res.json()) as { data: { id: string; filename: string; size_bytes: number } };
  return body.data;
}


// ---------- 代码浏览(T-122/T-134) ----------

/** 树条目。 */
export interface RepoTreeEntry {
  name: string;
  kind: "tree" | "blob";
  oid: string;
  size?: number;
}

/** 提交条目。 */
export interface RepoCommit {
  id: string;
  summary: string;
  author_name: string;
  author_email: string;
  time: number;
}

/** GET /api/repos/{owner}/{key}/tree */
export async function repoTree(
  owner: string,
  key: string,
  reference: string,
  path = "",
  baseUrl = "",
): Promise<RepoTreeEntry[]> {
  const res = await fetch(
    `${baseUrl}/api/repos/${owner}/${key}/tree?ref=${encodeURIComponent(reference)}&path=${encodeURIComponent(path)}`,
    { headers: { Authorization: `Bearer ${getToken() ?? ""}` } },
  );
  if (!res.ok) throw new ApiError("GIT-404", res.statusText, res.status);
  const body = (await res.json()) as { data: RepoTreeEntry[] };
  return body.data;
}

/** GET /api/repos/{owner}/{key}/commits */
export async function repoCommits(
  owner: string,
  key: string,
  reference: string,
  limit = 50,
  baseUrl = "",
): Promise<RepoCommit[]> {
  const res = await fetch(
    `${baseUrl}/api/repos/${owner}/${key}/commits?ref=${encodeURIComponent(reference)}&limit=${limit}`,
    { headers: { Authorization: `Bearer ${getToken() ?? ""}` } },
  );
  if (!res.ok) throw new ApiError("GIT-404", res.statusText, res.status);
  const body = (await res.json()) as { data: RepoCommit[] };
  return body.data;
}

/** GET /api/repos/{owner}/{key}/blob */
export async function repoBlob(
  owner: string,
  key: string,
  reference: string,
  path: string,
  baseUrl = "",
): Promise<string> {
  const res = await fetch(
    `${baseUrl}/api/repos/${owner}/${key}/blob?ref=${encodeURIComponent(reference)}&path=${encodeURIComponent(path)}`,
    { headers: { Authorization: `Bearer ${getToken() ?? ""}` } },
  );
  if (!res.ok) throw new ApiError("GIT-404", res.statusText, res.status);
  return res.text();
}
