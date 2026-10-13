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
  const payload = await request<{ data?: MeInfo }>("/api/me", baseUrl, {
    headers: { Authorization: `Bearer ${getToken()}` },
  });
  return payload.data ?? (payload as unknown as MeInfo);
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
  const payload = await request<{ data?: MyItem[] }>("/api/dashboard/my-items", baseUrl, {
    headers: { Authorization: `Bearer ${getToken() ?? ""}` },
  });
  return payload.data ?? (payload as unknown as MyItem[]);
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


// ---------- 平台管理(T-123/T-135) ----------

/** 平台用户(管理视图)。 */
export interface AdminUser {
  id: string;
  username: string;
  display_name: string;
  email: string;
  status: string;
  source: string;
  is_platform_admin: boolean;
}

/** GET /api/admin/users(平台管理员)。 */
export async function adminListUsers(baseUrl = ""): Promise<AdminUser[]> {
  const payload = await request<{ data?: AdminUser[] }>("/api/admin/users", baseUrl, {
    headers: { Authorization: `Bearer ${getToken() ?? ""}` },
  });
  return payload.data ?? (payload as unknown as AdminUser[]);
}

/** POST /api/admin/users/{id}/status(平台管理员)。 */
export async function adminSetUserStatus(
  id: string,
  status: "active" | "disabled",
  baseUrl = "",
): Promise<void> {
  const res = await fetch(`${baseUrl}/api/admin/users/${id}/status`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${getToken() ?? ""}`,
    },
    body: JSON.stringify({ status }),
  });
  if (!res.ok) {
    const errBody = (await res.json().catch(() => null)) as {
      code?: string;
      message?: string;
    } | null;
    throw new ApiError(
      errBody?.code ?? "ORG-500",
      errBody?.message ?? res.statusText,
      res.status,
    );
  }
}

/** GET /api/dashboard/counts */
export async function dashboardCounts(
  baseUrl = "",
): Promise<{ user_id: string; display_name: string; platform_users: number; is_platform_admin: boolean }> {
  return request("/api/dashboard/counts", baseUrl, {
    headers: { Authorization: `Bearer ${getToken() ?? ""}` },
  });
}

// ---------- PR 评审域(T-203/T-204) ----------

export interface PullRequest {
  id: string;
  number: number;
  title: string;
  body: string;
  source_branch: string;
  target_branch: string;
  author: string;
  state: string;
  draft: boolean;
  merge_method: string | null;
  merge_commit: string | null;
  created_at: string;
  updated_at: string;
}

export interface PrComment {
  id: string;
  author: string;
  body: string;
  file_path: string | null;
  line: number | null;
  side: string | null;
  anchor_sha: string | null;
  created_at: string;
}

export interface ReviewSummary {
  approvals: string[];
  changes_requested: string[];
  combined: string;
}

export interface StatusSummary {
  combined: string;
  checks: { context: string; state: string; description: string; target_url: string | null }[];
}

export interface ReviewerEntry {
  reviewer: string;
  added_by: string | null;
  verdict: string | null;
  created_at: string;
}

async function prRequest<T>(path: string, method: string, body?: unknown, baseUrl = ""): Promise<T> {
  const token = getToken();
  const res = await fetch(`${baseUrl}${path}`, {
    method,
    headers: {
      ...(body !== undefined ? { "Content-Type": "application/json" } : {}),
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  if (!res.ok) {
    const err = (await res.json().catch(() => null)) as { code?: string; message?: string } | null;
    throw new ApiError(err?.code ?? "HTTP", err?.message ?? res.statusText, res.status);
  }
  const payload = (await res.json()) as { data?: T };
  return payload.data as T;
}

/** GET /api/repos/{owner}/{key}/pulls */
export function listPulls(owner: string, key: string, state = "", baseUrl = ""): Promise<PullRequest[]> {
  return prRequest<PullRequest[]>(`/api/repos/${owner}/${key}/pulls${state ? `?state=${state}` : ""}`, "GET", undefined, baseUrl);
}

/** POST /api/repos/{owner}/{key}/pulls */
export function createPull(
  owner: string,
  key: string,
  input: { title: string; body?: string; source_branch: string; target_branch?: string; draft?: boolean },
  baseUrl = "",
): Promise<PullRequest> {
  return prRequest<PullRequest>(`/api/repos/${owner}/${key}/pulls`, "POST", input, baseUrl);
}

/** GET …/pulls/{number} */
export function getPull(owner: string, key: string, number: number, baseUrl = ""): Promise<PullRequest> {
  return prRequest<PullRequest>(`/api/repos/${owner}/${key}/pulls/${number}`, "GET", undefined, baseUrl);
}

/** GET …/pulls/{number}/diff */
export async function getPullDiff(owner: string, key: string, number: number, baseUrl = ""): Promise<string> {
  const token = getToken();
  const res = await fetch(`${baseUrl}/api/repos/${owner}/${key}/pulls/${number}/diff`, {
    headers: token ? { Authorization: `Bearer ${token}` } : {},
  });
  if (!res.ok) throw new ApiError("GIT-500", res.statusText, res.status);
  return res.text();
}

/** GET …/pulls/{number}/comments */
export function listPrComments(owner: string, key: string, number: number, baseUrl = ""): Promise<PrComment[]> {
  return prRequest<PrComment[]>(`/api/repos/${owner}/${key}/pulls/${number}/comments`, "GET", undefined, baseUrl);
}

/** POST …/pulls/{number}/comments */
export function addPrComment(
  owner: string,
  key: string,
  number: number,
  input: { body: string; file_path?: string; line?: number; side?: string },
  baseUrl = "",
): Promise<PrComment> {
  return prRequest<PrComment>(`/api/repos/${owner}/${key}/pulls/${number}/comments`, "POST", input, baseUrl);
}

/** POST …/pulls/{number}/reviews */
export function postReview(
  owner: string,
  key: string,
  number: number,
  verdict: "approved" | "changes_requested" | "comment",
  body = "",
  baseUrl = "",
): Promise<unknown> {
  return prRequest(`/api/repos/${owner}/${key}/pulls/${number}/reviews`, "POST", { verdict, body }, baseUrl);
}

/** GET …/pulls/{number}/review-summary */
export function reviewSummary(owner: string, key: string, number: number, baseUrl = ""): Promise<ReviewSummary> {
  return prRequest<ReviewSummary>(`/api/repos/${owner}/${key}/pulls/${number}/review-summary`, "GET", undefined, baseUrl);
}

/** GET …/pulls/{number}/status */
export function prStatus(owner: string, key: string, number: number, baseUrl = ""): Promise<StatusSummary> {
  return prRequest<StatusSummary>(`/api/repos/${owner}/${key}/pulls/${number}/status`, "GET", undefined, baseUrl);
}

/** GET …/pulls/{number}/reviewers */
export function listReviewers(owner: string, key: string, number: number, baseUrl = ""): Promise<ReviewerEntry[]> {
  return prRequest<ReviewerEntry[]>(`/api/repos/${owner}/${key}/pulls/${number}/reviewers`, "GET", undefined, baseUrl);
}

/** POST …/pulls/{number}/merge */
export function mergePull(
  owner: string,
  key: string,
  number: number,
  method: "merge" | "squash" | "rebase",
  baseUrl = "",
): Promise<PullRequest> {
  return prRequest<PullRequest>(`/api/repos/${owner}/${key}/pulls/${number}/merge`, "POST", { method }, baseUrl);
}

/** POST …/pulls/{number}/close | reopen */
export function transitionPull(
  owner: string,
  key: string,
  number: number,
  action: "close" | "reopen",
  baseUrl = "",
): Promise<PullRequest> {
  return prRequest<PullRequest>(`/api/repos/${owner}/${key}/pulls/${number}/${action}`, "POST", undefined, baseUrl);
}

// ---------- Tag / Release(T-208) ----------

export interface TagBrief {
  name: string;
  commit_sha: string;
  created_at: string;
}

export interface ReleaseInfo {
  tag: string;
  name: string;
  body: string;
  notes_auto: boolean;
  assets: { filename: string; size: number }[];
  created_at: string;
}

/** GET …/tags */
export function listTags(owner: string, key: string, baseUrl = ""): Promise<TagBrief[]> {
  return prRequest<TagBrief[]>(`/api/repos/${owner}/${key}/tags`, "GET", undefined, baseUrl);
}

/** POST …/tags */
export function createTag(
  owner: string,
  key: string,
  input: { name: string; message: string; target?: string },
  baseUrl = "",
): Promise<{ name: string }> {
  return prRequest<{ name: string }>(`/api/repos/${owner}/${key}/tags`, "POST", input, baseUrl);
}

/** GET …/releases */
export function listReleases(owner: string, key: string, baseUrl = ""): Promise<ReleaseInfo[]> {
  return prRequest<ReleaseInfo[]>(`/api/repos/${owner}/${key}/releases`, "GET", undefined, baseUrl);
}

/** POST …/releases */
export function publishRelease(
  owner: string,
  key: string,
  input: { tag_name: string; name?: string; body?: string; previous_tag?: string },
  baseUrl = "",
): Promise<ReleaseInfo> {
  return prRequest<ReleaseInfo>(`/api/repos/${owner}/${key}/releases`, "POST", input, baseUrl);
}

/** GET …/branch-protection */
export function listBranchProtection(owner: string, key: string, baseUrl = ""): Promise<unknown[]> {
  return prRequest<unknown[]>(`/api/repos/${owner}/${key}/branch-protection`, "GET", undefined, baseUrl);
}

/** PUT …/branch-protection */
export function setBranchProtection(
  owner: string,
  key: string,
  input: {
    branch_pattern: string;
    min_approvals: number;
    required_contexts: string[];
    block_force_push: boolean;
    block_direct_push: boolean;
    require_code_owner: boolean;
  },
  baseUrl = "",
): Promise<unknown> {
  return prRequest(`/api/repos/${owner}/${key}/branch-protection`, "PUT", input, baseUrl);
}

// ---------- 全文搜索 / 需求树 / 报表(T-213/T-212/T-215) ----------

export interface SearchHit {
  id: string;
  kind: string;
  project_id: string | null;
  title: string;
  snippet: string;
  score: number;
}

/** GET /api/search */
export function fetchSearch(q: string, kind?: string, limit = 20, baseUrl = ""): Promise<SearchHit[]> {
  const params = new URLSearchParams({ q, ...(kind ? { kind } : {}), limit: String(limit) });
  return prRequest<SearchHit[]>(`/api/search?${params}`, "GET", undefined, baseUrl);
}

export interface TreeItem {
  id: string;
  display_key: string;
  item_type: string;
  status: string;
  title: string;
  children: TreeItem[];
}

/** GET /api/projects/{key}/items/tree */
export function fetchItemsTree(projectKey: string, baseUrl = ""): Promise<TreeItem[]> {
  return prRequest<TreeItem[]>(`/api/projects/${projectKey}/items/tree`, "GET", undefined, baseUrl);
}

export interface SprintSummary {
  id: string;
  name: string;
  status: string;
}

/** GET /api/projects/{key}/sprints */
export function listSprints(projectKey: string, baseUrl = ""): Promise<SprintSummary[]> {
  return prRequest<SprintSummary[]>(`/api/projects/${projectKey}/sprints`, "GET", undefined, baseUrl);
}

/** GET …/reports/{kind}(bugs/throughput/cfd 序列;load 负载;sprint 迭代报告) */
export function fetchReport(
  projectKey: string,
  kind: "bugs" | "throughput" | "cfd" | "load",
  days = 14,
  baseUrl = "",
): Promise<unknown> {
  return prRequest(`/api/projects/${projectKey}/reports/${kind}?days=${days}`, "GET", undefined, baseUrl);
}

/** GET …/reports/sprint/{sprintId} */
export function fetchSprintReport(
  projectKey: string,
  sprintId: string,
  baseUrl = "",
): Promise<{ total: number; done: number; open: number; points_total: number; points_done: number; done_ratio: number }> {
  return prRequest(`/api/projects/${projectKey}/reports/sprint/${sprintId}`, "GET", undefined, baseUrl);
}

/** 工作项(项目内列表,列表视图数据源;字段与后端 ItemResponse 对齐) */
export interface ProjectItem {
  id: string;
  number: number;
  item_type: string;
  title: string;
  status: string;
  priority: string;
  assignee: string | null;
  custom_fields: Record<string, unknown>;
  created_at: string;
  updated_at: string;
}

/** GET /api/projects/{key}/items */
export function listProjectItems(projectKey: string, baseUrl = ""): Promise<ProjectItem[]> {
  return prRequest<ProjectItem[]>(`/api/projects/${projectKey}/items`, "GET", undefined, baseUrl);
}

// ---------- 工作项详情 / 评论(T-132) ----------

/** 工作项详情(详情页全字段) */
export interface ItemDetail {
  id: string;
  number: number;
  project_id: string;
  item_type: string;
  title: string;
  description: string;
  status: string;
  priority: string;
  assignee: string | null;
  reporter: string | null;
  parent_id: string | null;
  custom_fields: Record<string, unknown>;
  created_at: string;
  updated_at: string;
}

/** GET /api/workitems/{id} */
export function fetchItemDetail(id: string, baseUrl = ""): Promise<ItemDetail> {
  return prRequest<ItemDetail>(`/api/workitems/${id}`, "GET", undefined, baseUrl);
}

/** GET /api/workitems/{id}/transitions(服务端状态机裁定) */
export function fetchItemTransitions(id: string, baseUrl = ""): Promise<string[]> {
  return prRequest<string[]>(`/api/workitems/${id}/transitions`, "GET", undefined, baseUrl);
}

/** 工作项流转 POST /api/workitems/{id}/transition */
export function transitionItem(id: string, to: string, baseUrl = ""): Promise<ItemDetail> {
  return prRequest<ItemDetail>(`/api/workitems/${id}/transition`, "POST", { to }, baseUrl);
}

/** 工作项评论 */
export interface ItemComment {
  id: string;
  work_item_id: string;
  author: string | null;
  body: string;
  created_at: string;
}

/** GET /api/workitems/{id}/comments */
export function listItemComments(id: string, baseUrl = ""): Promise<ItemComment[]> {
  return prRequest<ItemComment[]>(`/api/workitems/${id}/comments`, "GET", undefined, baseUrl);
}

/** POST /api/workitems/{id}/comments */
export function addItemComment(id: string, body: string, baseUrl = ""): Promise<ItemComment> {
  return prRequest<ItemComment>(`/api/workitems/${id}/comments`, "POST", { body }, baseUrl);
}

/** DELETE /api/workitems/{id}/comments/{commentId} */
export function deleteItemComment(id: string, commentId: string, baseUrl = ""): Promise<void> {
  return prRequest<void>(`/api/workitems/${id}/comments/${commentId}`, "DELETE", undefined, baseUrl);
}

/** 工作项关联提交 */
export interface ItemCommit {
  sha: string;
  subject: string;
  repo: string;
  ref: string;
  created_at: string;
}

/** GET /api/workitems/{id}/commits */
export function fetchItemCommits(id: string, baseUrl = ""): Promise<ItemCommit[]> {
  return prRequest<ItemCommit[]>(`/api/workitems/${id}/commits`, "GET", undefined, baseUrl);
}

// ---------- 组织架构 / 审计(ORG-01 / ORG-05,T-135) ----------

/** 组织 */
export interface OrgInfo {
  id: string;
  name: string;
  team_count: number;
  created_at: string;
}

/** GET /api/admin/orgs */
export function listOrgs(baseUrl = ""): Promise<OrgInfo[]> {
  return prRequest<OrgInfo[]>("/api/admin/orgs", "GET", undefined, baseUrl);
}

/** POST /api/admin/orgs */
export function createOrg(name: string, baseUrl = ""): Promise<OrgInfo> {
  return prRequest<OrgInfo>("/api/admin/orgs", "POST", { name }, baseUrl);
}

/** 团队 */
export interface TeamInfo {
  id: string;
  org_id: string;
  name: string;
  member_count: number;
  created_at: string;
}

/** GET /api/admin/orgs/{id}/teams */
export function listTeams(orgId: string, baseUrl = ""): Promise<TeamInfo[]> {
  return prRequest<TeamInfo[]>(`/api/admin/orgs/${orgId}/teams`, "GET", undefined, baseUrl);
}

/** POST /api/admin/orgs/{id}/teams */
export function createTeam(orgId: string, name: string, baseUrl = ""): Promise<TeamInfo> {
  return prRequest<TeamInfo>(`/api/admin/orgs/${orgId}/teams`, "POST", { name }, baseUrl);
}

/** 团队成员 */
export interface TeamMemberInfo {
  user_id: string;
  username: string;
  display_name: string;
  joined_at: string;
}

/** GET /api/admin/teams/{id}/members */
export function listTeamMembers(teamId: string, baseUrl = ""): Promise<TeamMemberInfo[]> {
  return prRequest<TeamMemberInfo[]>(`/api/admin/teams/${teamId}/members`, "GET", undefined, baseUrl);
}

/** POST /api/admin/teams/{id}/members */
export function addTeamMember(teamId: string, username: string, baseUrl = ""): Promise<void> {
  return prRequest<void>(`/api/admin/teams/${teamId}/members`, "POST", { username }, baseUrl);
}

/** DELETE /api/admin/teams/{id}/members/{userId} */
export function removeTeamMember(teamId: string, userId: string, baseUrl = ""): Promise<void> {
  return prRequest<void>(`/api/admin/teams/${teamId}/members/${userId}`, "DELETE", undefined, baseUrl);
}

/** 审计记录 */
export interface AuditRecord {
  id: string;
  actor: string | null;
  action: string;
  resource_kind: string | null;
  resource_id: string | null;
  detail: Record<string, unknown>;
  at: string;
}

/** GET /api/admin/audit */
export function listAudit(limit = 100, baseUrl = ""): Promise<AuditRecord[]> {
  return prRequest<AuditRecord[]>(`/api/admin/audit?limit=${limit}`, "GET", undefined, baseUrl);
}

// ---------- 效能报表 / 外部链接器(T-308 / MS-02,03) ----------

/** 时长分布(秒)。 */
export interface DurationStats {
  count: number;
  median_seconds: number | null;
  p85_seconds: number | null;
}

/** 缺陷逃逸。 */
export interface DefectEscape {
  total: number;
  escaped: number;
  rate_percent: number;
}

/** 项目效能(交付周期 + 缺陷逃逸) */
export interface ProjectEfficiency {
  delivery_cycle: DurationStats;
  defect_escape: DefectEscape;
}

/** GET /api/projects/{key}/reports/efficiency */
export function fetchProjectEfficiency(key: string, baseUrl = ""): Promise<ProjectEfficiency> {
  return prRequest<ProjectEfficiency>(`/api/projects/${key}/reports/efficiency`, "GET", undefined, baseUrl);
}

/** 仓库 PR 效能 */
export interface PrMetrics {
  pr_cycle: DurationStats;
  review_response: DurationStats;
}

/** GET /api/repos/{owner}/{key}/reports/pr-metrics */
export function fetchPrMetrics(owner: string, key: string, baseUrl = ""): Promise<PrMetrics> {
  return prRequest<PrMetrics>(`/api/repos/${owner}/${key}/reports/pr-metrics`, "GET", undefined, baseUrl);
}

/** DORA 四指标 */
export interface DoraSummary {
  days: number;
  deployment_frequency: { deployments: number; days: number; per_week: number };
  lead_time: DurationStats;
  change_failure_rate: { deployments: number; failures: number; rate_percent: number };
  mttr: { incidents: number; unresolved: number; mean_seconds: number | null };
}

/** GET /api/repos/{owner}/{key}/dora */
export function fetchDora(owner: string, key: string, days = 30, baseUrl = ""): Promise<DoraSummary> {
  return prRequest<DoraSummary>(`/api/repos/${owner}/${key}/dora?days=${days}`, "GET", undefined, baseUrl);
}

/** 解析后的外部链接 */
export interface ResolvedLink {
  name: string;
  url: string;
}

/** GET /api/repos/{owner}/{key}/external-links */
export function fetchExternalLinks(
  owner: string,
  key: string,
  sha = "",
  branch = "",
  baseUrl = "",
): Promise<ResolvedLink[]> {
  const qs = new URLSearchParams();
  if (sha) qs.set("sha", sha);
  if (branch) qs.set("branch", branch);
  const suffix = qs.toString() ? `?${qs.toString()}` : "";
  return prRequest<ResolvedLink[]>(`/api/repos/${owner}/${key}/external-links${suffix}`, "GET", undefined, baseUrl);
}

// ---------- 工时(T-318 / PM-09) ----------

/** 工时记录 */
export interface WorklogEntry {
  id: string;
  work_item_id: string;
  author: string | null;
  minutes: number;
  note: string;
  spent_on: string;
  created_at: string;
}

/** 工时汇总 */
export interface WorklogSummary {
  logged_minutes: number;
  estimate_minutes: number | null;
  entries: number;
  by_author: Record<string, number>;
}

/** GET /api/workitems/{id}/worklogs */
export function listWorklogs(itemId: string, baseUrl = ""): Promise<WorklogEntry[]> {
  return prRequest<WorklogEntry[]>(`/api/workitems/${itemId}/worklogs`, "GET", undefined, baseUrl);
}

/** GET /api/workitems/{id}/worklogs/summary */
export function fetchWorklogSummary(itemId: string, baseUrl = ""): Promise<WorklogSummary> {
  return prRequest<WorklogSummary>(`/api/workitems/${itemId}/worklogs/summary`, "GET", undefined, baseUrl);
}

/** POST /api/workitems/{id}/worklogs */
export function logWork(
  itemId: string,
  minutes: number,
  note = "",
  spentOn = "",
  baseUrl = "",
): Promise<WorklogEntry> {
  return prRequest<WorklogEntry>(
    `/api/workitems/${itemId}/worklogs`,
    "POST",
    { minutes, note, ...(spentOn ? { spent_on: spentOn } : {}) },
    baseUrl,
  );
}

/** DELETE /api/workitems/{id}/worklogs/{worklogId} */
export function deleteWorklog(itemId: string, worklogId: string, baseUrl = ""): Promise<void> {
  return prRequest<void>(`/api/workitems/${itemId}/worklogs/${worklogId}`, "DELETE", undefined, baseUrl);
}

// ---------- 应用中心(T-310 / PAAS-01,02) ----------

/** 应用 */
export interface AppInfo {
  id: string;
  key: string;
  name: string;
  description: string;
  owner: string | null;
  created_at: string;
}

/** 环境 */
export interface EnvInfo {
  id: string;
  app_id: string;
  name: string;
  kind: string;
  production: boolean;
  created_at: string;
}

/** 部署目标 */
export interface DeployTargetInfo {
  id: string;
  env_id: string;
  name: string;
  host: string;
  port: number;
  labels: string[];
  created_at: string;
}

/** 环境变量(敏感项 value 为 null) */
export interface EnvVarInfo {
  id: string;
  key: string;
  value: string | null;
  is_secret: boolean;
}

/** 应用聚合视图 */
export interface AppAggregate {
  application: {
    id: string;
    key: string;
    name: string;
    description: string;
    owner: string | null;
    created_at: string;
  };
  environments: Array<{
    id: string;
    name: string;
    kind: string;
    target_count: number;
    var_count: number;
    production: boolean;
  }>;
  repos: Array<{ id: string; full_name: string }>;
}

/** GET /api/apps */
export function listApps(baseUrl = ""): Promise<AppInfo[]> {
  return prRequest<AppInfo[]>("/api/apps", "GET", undefined, baseUrl);
}

/** POST /api/apps */
export function createApp(key: string, name: string, description = "", baseUrl = ""): Promise<AppInfo> {
  return prRequest<AppInfo>("/api/apps", "POST", { key, name, description }, baseUrl);
}

/** GET /api/apps/{key} */
export function fetchAppAggregate(key: string, baseUrl = ""): Promise<AppAggregate> {
  return prRequest<AppAggregate>(`/api/apps/${key}`, "GET", undefined, baseUrl);
}

/** POST /api/apps/{key}/environments */
export function createAppEnv(key: string, name: string, kind: string, baseUrl = ""): Promise<EnvInfo> {
  return prRequest<EnvInfo>(`/api/apps/${key}/environments`, "POST", { name, kind }, baseUrl);
}

/** DELETE /api/apps/{key}/environments/{envId} */
export function deleteAppEnv(key: string, envId: string, baseUrl = ""): Promise<void> {
  return prRequest<void>(`/api/apps/${key}/environments/${envId}`, "DELETE", undefined, baseUrl);
}

/** GET /api/apps/{key}/environments/{envId}/targets */
export function listDeployTargets(key: string, envId: string, baseUrl = ""): Promise<DeployTargetInfo[]> {
  return prRequest<DeployTargetInfo[]>(`/api/apps/${key}/environments/${envId}/targets`, "GET", undefined, baseUrl);
}

/** POST /api/apps/{key}/environments/{envId}/targets */
export function addDeployTarget(
  key: string,
  envId: string,
  body: { name: string; host: string; port?: number; labels?: string[] },
  baseUrl = "",
): Promise<DeployTargetInfo> {
  return prRequest<DeployTargetInfo>(`/api/apps/${key}/environments/${envId}/targets`, "POST", body, baseUrl);
}

/** DELETE /api/apps/{key}/environments/{envId}/targets/{targetId} */
export function removeDeployTarget(key: string, envId: string, targetId: string, baseUrl = ""): Promise<void> {
  return prRequest<void>(`/api/apps/${key}/environments/${envId}/targets/${targetId}`, "DELETE", undefined, baseUrl);
}

/** GET /api/apps/{key}/environments/{envId}/vars */
export function listEnvVars(key: string, envId: string, baseUrl = ""): Promise<EnvVarInfo[]> {
  return prRequest<EnvVarInfo[]>(`/api/apps/${key}/environments/${envId}/vars`, "GET", undefined, baseUrl);
}

/** POST /api/apps/{key}/environments/{envId}/vars */
export function upsertEnvVar(
  key: string,
  envId: string,
  body: { key: string; value: string; is_secret?: boolean },
  baseUrl = "",
): Promise<EnvVarInfo> {
  return prRequest<EnvVarInfo>(`/api/apps/${key}/environments/${envId}/vars`, "POST", body, baseUrl);
}

/** DELETE /api/apps/{key}/environments/{envId}/vars/{varId} */
export function deleteEnvVar(key: string, envId: string, varId: string, baseUrl = ""): Promise<void> {
  return prRequest<void>(`/api/apps/${key}/environments/${envId}/vars/${varId}`, "DELETE", undefined, baseUrl);
}

/** POST /api/apps/{key}/repos */
export function linkAppRepo(key: string, repoId: string, baseUrl = ""): Promise<void> {
  return prRequest<void>(`/api/apps/${key}/repos`, "POST", { repo_id: repoId }, baseUrl);
}

/** DELETE /api/apps/{key}/repos */
export function unlinkAppRepo(key: string, repoId: string, baseUrl = ""): Promise<void> {
  return prRequest<void>(`/api/apps/${key}/repos`, "DELETE", { repo_id: repoId }, baseUrl);
}

// ---------- 服务规格(T-312 / PAAS-03) ----------

/** 服务规格(声明式) */
export interface ServiceSpec {
  image: string;
  replicas: number;
  ports: Array<{ container: number; protocol: string }>;
  env: Array<{ name: string; value: string }>;
  volumes: Array<{ name: string; mount_path: string; size_mb: number }>;
  healthcheck: {
    path: string;
    port: number;
    interval_seconds: number;
    timeout_seconds: number;
  } | null;
}

/** 规格版本 */
export interface SpecVersionInfo {
  version: number;
  spec: ServiceSpec;
  source: string;
  created_by: string | null;
  created_at: string;
}

/** GET /api/apps/{key}/environments/{envId}/spec */
export function fetchServiceSpec(
  key: string,
  envId: string,
  baseUrl = "",
): Promise<SpecVersionInfo | null> {
  return prRequest<SpecVersionInfo | null>(
    `/api/apps/${key}/environments/${envId}/spec`,
    "GET",
    undefined,
    baseUrl,
  );
}

/** PUT /api/apps/{key}/environments/{envId}/spec(规格即代码:JSON/YAML 文本) */
export function applyServiceSpec(
  key: string,
  envId: string,
  spec: string,
  baseUrl = "",
): Promise<SpecVersionInfo> {
  return prRequest<SpecVersionInfo>(
    `/api/apps/${key}/environments/${envId}/spec`,
    "PUT",
    { spec },
    baseUrl,
  );
}

/** GET /api/apps/{key}/environments/{envId}/spec/versions */
export function listServiceSpecVersions(
  key: string,
  envId: string,
  baseUrl = "",
): Promise<SpecVersionInfo[]> {
  return prRequest<SpecVersionInfo[]>(
    `/api/apps/${key}/environments/${envId}/spec/versions`,
    "GET",
    undefined,
    baseUrl,
  );
}

// ---------- 配置与密钥(T-317 / PAAS-06) ----------

/** 配置项(敏感项 value 为 null) */
export interface ConfigItemView {
  key: string;
  value: string | null;
  secret: boolean;
}

/** 配置版本 */
export interface ConfigVersionInfo {
  version: number;
  items: ConfigItemView[];
  source: string;
  created_by: string | null;
  created_at: string;
}

/** GET /api/apps/{key}/environments/{envId}/config */
export function fetchAppConfig(
  key: string,
  envId: string,
  baseUrl = "",
): Promise<ConfigVersionInfo | null> {
  return prRequest<ConfigVersionInfo | null>(
    `/api/apps/${key}/environments/${envId}/config`,
    "GET",
    undefined,
    baseUrl,
  );
}

/** PUT /api/apps/{key}/environments/{envId}/config(敏感项写 {value, secret: true}) */
export function applyAppConfig(
  key: string,
  envId: string,
  config: string,
  baseUrl = "",
): Promise<ConfigVersionInfo> {
  return prRequest<ConfigVersionInfo>(
    `/api/apps/${key}/environments/${envId}/config`,
    "PUT",
    { config },
    baseUrl,
  );
}

/** GET /api/apps/{key}/environments/{envId}/config/versions */
export function listAppConfigVersions(
  key: string,
  envId: string,
  baseUrl = "",
): Promise<ConfigVersionInfo[]> {
  return prRequest<ConfigVersionInfo[]>(
    `/api/apps/${key}/environments/${envId}/config/versions`,
    "GET",
    undefined,
    baseUrl,
  );
}

/** POST /api/apps/{key}/environments/{envId}/config/reveal(单键解密,留审计) */
export function revealAppConfigKey(
  key: string,
  envId: string,
  itemKey: string,
  baseUrl = "",
): Promise<{ key: string; value: string; was_secret: boolean }> {
  return prRequest<{ key: string; value: string; was_secret: boolean }>(
    `/api/apps/${key}/environments/${envId}/config/reveal`,
    "POST",
    { key: itemKey },
    baseUrl,
  );
}

// ---------- 域名绑定与网关路由(T-315 / PAAS-05) ----------

/** 域名绑定 */
export interface DomainInfo {
  id: string;
  env_id: string;
  host: string;
  kind: string;
  tls_enabled: boolean;
  cert_ref: string;
  created_at: string;
}

/** GET /api/apps/{key}/environments/{envId}/domains */
export function listAppDomains(key: string, envId: string, baseUrl = ""): Promise<DomainInfo[]> {
  return prRequest<DomainInfo[]>(`/api/apps/${key}/environments/${envId}/domains`, "GET", undefined, baseUrl);
}

/** POST /api/apps/{key}/environments/{envId}/domains */
export function bindAppDomain(
  key: string,
  envId: string,
  host: string,
  tlsEnabled = true,
  baseUrl = "",
): Promise<DomainInfo> {
  return prRequest<DomainInfo>(
    `/api/apps/${key}/environments/${envId}/domains`,
    "POST",
    { host, tls_enabled: tlsEnabled },
    baseUrl,
  );
}

/** POST /api/apps/{key}/environments/{envId}/domains/auto */
export function autoAssignDomain(key: string, envId: string, baseUrl = ""): Promise<DomainInfo> {
  return prRequest<DomainInfo>(
    `/api/apps/${key}/environments/${envId}/domains/auto`,
    "POST",
    undefined,
    baseUrl,
  );
}

/** DELETE /api/apps/{key}/environments/{envId}/domains/{domainId} */
export function unbindAppDomain(key: string, envId: string, domainId: string, baseUrl = ""): Promise<void> {
  return prRequest<void>(
    `/api/apps/${key}/environments/${envId}/domains/${domainId}`,
    "DELETE",
    undefined,
    baseUrl,
  );
}

/** GET /api/apps/{key}/environments/{envId}/gateway/nginx(纯文本配置) */
export function fetchGatewayNginx(key: string, envId: string, baseUrl = ""): Promise<string> {
  return prRequest<string>(`/api/apps/${key}/environments/${envId}/gateway/nginx`, "GET", undefined, baseUrl);
}

/** GET /api/apps/{key}/environments/{envId}/gateway/apisix */
export function fetchGatewayApisix(
  key: string,
  envId: string,
  baseUrl = "",
): Promise<{ route_id: string; admin_path: string; body: unknown }> {
  return prRequest<{ route_id: string; admin_path: string; body: unknown }>(
    `/api/apps/${key}/environments/${envId}/gateway/apisix`,
    "GET",
    undefined,
    baseUrl,
  );
}

/** POST /api/apps/{key}/environments/{envId}/gateway/apply */
export function applyGateway(
  key: string,
  envId: string,
  baseUrl = "",
): Promise<{ path: string; bytes: number }> {
  return prRequest<{ path: string; bytes: number }>(
    `/api/apps/${key}/environments/${envId}/gateway/apply`,
    "POST",
    undefined,
    baseUrl,
  );
}

// ---------- 证书(T-316 / PAAS-07) ----------

/** 证书记录 */
export interface CertInfo {
  id: string;
  env_id: string;
  cert_ref: string;
  domains: string[];
  provider: string;
  status: string;
  not_after: string | null;
  issued_at: string;
}

/** GET /api/apps/{key}/environments/{envId}/certificates */
export function listCertificates(key: string, envId: string, baseUrl = ""): Promise<CertInfo[]> {
  return prRequest<CertInfo[]>(`/api/apps/${key}/environments/${envId}/certificates`, "GET", undefined, baseUrl);
}

/** POST /api/apps/{key}/environments/{envId}/certificates(ACME 签发/续期) */
export function issueCertificate(key: string, envId: string, baseUrl = ""): Promise<CertInfo> {
  return prRequest<CertInfo>(`/api/apps/${key}/environments/${envId}/certificates`, "POST", undefined, baseUrl);
}
