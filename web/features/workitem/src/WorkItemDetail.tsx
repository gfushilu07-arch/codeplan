import { useCallback, useEffect, useState } from "react";
import {
  addItemComment,
  deleteItemComment,
  fetchItemCommits,
  fetchItemDetail,
  fetchItemTransitions,
  deleteWorklog,
  fetchWorklogSummary,
  listItemComments,
  listWorklogs,
  logWork,
  transitionItem,
  type ItemComment,
  type ItemCommit,
  type ItemDetail,
  type WorklogEntry,
  type WorklogSummary,
} from "@codeplan/api-client";
import { statusBadgeClass, statusLabel } from "./status";

const TYPE_LABEL: Record<string, string> = {
  epic: "史诗",
  story: "需求",
  task: "任务",
  bug: "缺陷",
};

const PRIORITY_LABEL: Record<string, string> = {
  highest: "最高",
  high: "高",
  medium: "中",
  low: "低",
  lowest: "最低",
};

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div style={{ display: "flex", gap: "var(--cp-space-3)", fontSize: 13, padding: "6px 0" }}>
      <span className="cp-muted" style={{ width: 72, flexShrink: 0 }}>{label}</span>
      <span style={{ minWidth: 0, color: "var(--cp-color-text)" }}>{children}</span>
    </div>
  );
}

/**
 * 工作项详情(T-132):状态流转(目标由服务端状态机裁定)+ 描述 + 关联提交 + 评论对话流。
 */
export function WorkItemDetail({ itemId }: { itemId: string }) {
  const [item, setItem] = useState<ItemDetail | null>(null);
  const [targets, setTargets] = useState<string[]>([]);
  const [comments, setComments] = useState<ItemComment[]>([]);
  const [commits, setCommits] = useState<ItemCommit[]>([]);
  const [worklogs, setWorklogs] = useState<WorklogEntry[]>([]);
  const [worklogSum, setWorklogSum] = useState<WorklogSummary | null>(null);
  const [wlMinutes, setWlMinutes] = useState("");
  const [wlNote, setWlNote] = useState("");
  const [wlDate, setWlDate] = useState("");
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      const [d, t, c, cm, wl, ws] = await Promise.all([
        fetchItemDetail(itemId),
        fetchItemTransitions(itemId).catch(() => [] as string[]),
        listItemComments(itemId).catch(() => [] as ItemComment[]),
        fetchItemCommits(itemId).catch(() => [] as ItemCommit[]),
        listWorklogs(itemId).catch(() => [] as WorklogEntry[]),
        fetchWorklogSummary(itemId).catch(() => null),
      ]);
      setItem(d);
      setTargets(t);
      setComments(c);
      setCommits(cm);
      setWorklogs(wl);
      setWorklogSum(ws);
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    }
  }, [itemId]);

  useEffect(() => {
    void load();
  }, [load]);

  const move = async (to: string) => {
    setBusy(true);
    try {
      await transitionItem(itemId, to);
      await load();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const send = async () => {
    if (!draft.trim()) return;
    setBusy(true);
    try {
      await addItemComment(itemId, draft.trim());
      setDraft("");
      await load();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const removeComment = async (commentId: string) => {
    try {
      await deleteItemComment(itemId, commentId);
      await load();
    } catch (e) {
      setError((e as Error).message);
    }
  };

  const submitWorklog = async () => {
    const minutes = Number(wlMinutes);
    if (!Number.isFinite(minutes) || minutes <= 0) return;
    setBusy(true);
    try {
      await logWork(itemId, minutes, wlNote, wlDate);
      setWlMinutes("");
      setWlNote("");
      setWlDate("");
      await load();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const removeWorklog = async (id: string) => {
    try {
      await deleteWorklog(itemId, id);
      await load();
    } catch (e) {
      setError((e as Error).message);
    }
  };

  if (error && !item) return <div className="cp-alert cp-alert-danger">{error}</div>;
  if (!item) {
    return (
      <div className="cp-page">
        <div className="cp-skeleton" style={{ height: 40, width: 320 }} />
        <div className="cp-skeleton" style={{ height: 200 }} />
      </div>
    );
  }

  const typeLabel = TYPE_LABEL[item.item_type] ?? item.item_type;

  return (
    <div className="cp-page">
      <nav className="cp-breadcrumb">
        <a href="#/items">工作项</a>
        <span className="sep">/</span>
        <span className="current">#{item.number}</span>
      </nav>

      <div className="cp-page-head">
        <div style={{ minWidth: 0 }}>
          <h1 className="cp-page-title">
            <span className="cp-muted" style={{ fontWeight: 400 }}>#{item.number}</span>
            {item.title}
          </h1>
          <div className="cp-row" style={{ marginTop: 8, flexWrap: "wrap" }}>
            <span className="cp-tag">{typeLabel}</span>
            <span className={statusBadgeClass(item.status)}>{statusLabel(item.status)}</span>
            <span className="cp-badge cp-badge-neutral">优先级 {PRIORITY_LABEL[item.priority] ?? item.priority}</span>
          </div>
        </div>
        <div className="cp-page-actions">
          <a className="cp-btn" href="#/items">返回列表</a>
          {targets.map((to) => (
            <button key={to} className="cp-btn-primary" disabled={busy} onClick={() => void move(to)}>
              → {statusLabel(to)}
            </button>
          ))}
          {targets.length === 0 && <span className="cp-muted" style={{ fontSize: 12 }}>无可用流转</span>}
        </div>
      </div>

      {error && <div className="cp-alert cp-alert-danger">{error}</div>}

      <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) 300px", gap: "var(--cp-space-6)", alignItems: "start" }}>
        {/* 主列:描述 + 关联提交 + 评论 */}
        <div className="cp-col" style={{ gap: "var(--cp-space-4)" }}>
          <div className="cp-panel">
            <div className="cp-panel-head">描述</div>
            <div className="cp-panel-body">
              {item.description.trim() ? (
                <div className="wiki-html" style={{ whiteSpace: "pre-wrap" }}>{item.description}</div>
              ) : (
                <div className="cp-muted" style={{ fontSize: 13 }}>暂无描述</div>
              )}
            </div>
          </div>

          {commits.length > 0 && (
            <div className="cp-panel">
              <div className="cp-panel-head">
                关联提交
                <span className="cp-badge cp-badge-neutral">{commits.length}</span>
              </div>
              <div>
                {commits.map((c) => (
                  <div className="cp-file-row" key={c.sha + c.ref} style={{ height: "auto", padding: "10px var(--cp-space-4)" }}>
                    <span className="cp-mono cp-muted">{c.sha.slice(0, 8)}</span>
                    <div style={{ minWidth: 0, flex: 1 }}>
                      <div className="cp-nowrap">{c.subject}</div>
                      <div className="cp-list-meta">
                        <span className="cp-mono">{c.repo}</span>
                        <span className="cp-mono">{c.ref}</span>
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}

          <div className="cp-panel">
            <div className="cp-panel-head">
              工时
              <span className="cp-badge cp-badge-neutral">
                累计 {Math.floor((worklogSum?.logged_minutes ?? 0) / 60)}h{(worklogSum?.logged_minutes ?? 0) % 60}m
              </span>
              {worklogSum?.estimate_minutes != null && (
                <span className="cp-muted" style={{ fontSize: 12 }}>
                  预估 {Math.floor(worklogSum.estimate_minutes / 60)}h{worklogSum.estimate_minutes % 60}m
                </span>
              )}
            </div>
            {worklogSum && worklogSum.estimate_minutes ? (
              <div style={{ padding: "var(--cp-space-3) var(--cp-space-4) 0" }}>
                <div className="cp-progress">
                  <span
                    style={{
                      width: `${Math.min(100, Math.round(((worklogSum.logged_minutes ?? 0) / worklogSum.estimate_minutes) * 100))}%`,
                    }}
                  />
                </div>
                <div className="cp-muted" style={{ fontSize: 12, marginTop: 4 }}>
                  已用 {Math.round(((worklogSum.logged_minutes ?? 0) / worklogSum.estimate_minutes) * 100)}% 预估
                </div>
              </div>
            ) : null}
            <div>
              {worklogs.length === 0 ? (
                <div className="cp-muted" style={{ fontSize: 13, padding: "var(--cp-space-4)" }}>
                  还没有工时记录。
                </div>
              ) : (
                worklogs.map((w) => (
                  <div
                    key={w.id}
                    className="cp-row"
                    style={{ padding: "var(--cp-space-2) var(--cp-space-4)", borderBottom: "1px solid var(--cp-color-border-light)", fontSize: 13 }}
                  >
                    <span className="cp-mono cp-muted" style={{ width: 84 }}>{w.spent_on}</span>
                    <b style={{ width: 64 }}>{Math.floor(w.minutes / 60)}h{w.minutes % 60}m</b>
                    <span className="cp-nowrap" style={{ flex: 1 }}>{w.note || "—"}</span>
                    <button className="cp-btn cp-btn-sm ghost" onClick={() => void removeWorklog(w.id)}>删除</button>
                  </div>
                ))
              )}
            </div>
            <div className="cp-panel-body" style={{ borderTop: "1px solid var(--cp-color-border-light)" }}>
              <div className="cp-toolbar">
                <input
                  type="number"
                  min={1}
                  max={1440}
                  placeholder="分钟"
                  value={wlMinutes}
                  onChange={(e) => setWlMinutes(e.target.value)}
                  style={{ width: 90 }}
                />
                <input
                  type="date"
                  value={wlDate}
                  onChange={(e) => setWlDate(e.target.value)}
                  style={{ width: 150 }}
                />
                <input
                  placeholder="做了什么"
                  value={wlNote}
                  onChange={(e) => setWlNote(e.target.value)}
                  style={{ flex: 1, minWidth: 160 }}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") void submitWorklog();
                  }}
                />
                <button className="cp-btn-primary" disabled={busy || !wlMinutes} onClick={() => void submitWorklog()}>
                  登记
                </button>
              </div>
            </div>
          </div>

          <div className="cp-panel">
            <div className="cp-panel-head">
              评论
              <span className="cp-badge cp-badge-neutral">{comments.length}</span>
            </div>
            <div>
              {comments.length === 0 ? (
                <div className="cp-muted" style={{ fontSize: 13, padding: "var(--cp-space-4)" }}>
                  还没有评论,写下第一条吧。
                </div>
              ) : (
                comments.map((c) => (
                  <div key={c.id} style={{ padding: "var(--cp-space-3) var(--cp-space-4)", borderBottom: "1px solid var(--cp-color-border-light)" }}>
                    <div className="cp-row" style={{ fontSize: 12 }}>
                      <span className="cp-avatar cp-avatar-sm">
                        {(c.author ?? "?").slice(0, 1).toUpperCase()}
                      </span>
                      <span className="cp-mono cp-muted">{(c.author ?? "系统").slice(0, 8)}</span>
                      <span className="cp-muted">{new Date(c.created_at).toLocaleString()}</span>
                      <div className="cp-spacer" />
                      <button className="cp-btn cp-btn-sm ghost" onClick={() => void removeComment(c.id)}>删除</button>
                    </div>
                    <div style={{ whiteSpace: "pre-wrap", marginTop: 8, fontSize: 14 }}>{c.body}</div>
                  </div>
                ))
              )}
            </div>
            <div className="cp-panel-body" style={{ borderTop: "1px solid var(--cp-color-border-light)" }}>
              <textarea
                rows={3}
                placeholder="写下你的评论…(Markdown)"
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                style={{ width: "100%" }}
              />
              <div className="cp-row" style={{ marginTop: "var(--cp-space-2)" }}>
                <button className="cp-btn-primary" disabled={busy || !draft.trim()} onClick={() => void send()}>
                  发表评论
                </button>
              </div>
            </div>
          </div>
        </div>

        {/* 侧栏:字段 */}
        <aside className="cp-panel">
          <div className="cp-panel-head">属性</div>
          <div className="cp-panel-body" style={{ paddingTop: 6 }}>
            <Field label="类型">{typeLabel}</Field>
            <Field label="状态">
              <span className={statusBadgeClass(item.status)}>{statusLabel(item.status)}</span>
            </Field>
            <Field label="优先级">{PRIORITY_LABEL[item.priority] ?? item.priority}</Field>
            <Field label="经办人">
              {item.assignee ? <span className="cp-mono">{item.assignee.slice(0, 8)}</span> : <span className="cp-muted">未指派</span>}
            </Field>
            <Field label="报告人">
              {item.reporter ? <span className="cp-mono">{item.reporter.slice(0, 8)}</span> : <span className="cp-muted">—</span>}
            </Field>
            {item.parent_id && (
              <Field label="父工作项">
                <a className="cp-mono" href={`#/item/${item.parent_id}`}>{item.parent_id.slice(0, 8)}</a>
              </Field>
            )}
            {typeof item.custom_fields?.severity === "string" && (
              <Field label="严重程度">{String(item.custom_fields.severity)}</Field>
            )}
            <Field label="创建于">{new Date(item.created_at).toLocaleString()}</Field>
            <Field label="更新于">{new Date(item.updated_at).toLocaleString()}</Field>
          </div>
        </aside>
      </div>
    </div>
  );
}
