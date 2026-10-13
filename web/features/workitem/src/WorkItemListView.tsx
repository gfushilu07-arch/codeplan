import { useEffect, useMemo, useState } from "react";
import {
  listProjectItems,
  listProjects,
  type ProjectItem,
  type ProjectSummary,
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

const STATUS_OPTIONS = ["todo", "in_progress", "in_review", "done", "open", "resolved", "closed"];

/**
 * 工作项列表视图(T-132):项目 + 类型/状态筛选 + 关键词,行点击进入详情。
 */
export function WorkItemListView() {
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [projectKey, setProjectKey] = useState<string>("");
  const [items, setItems] = useState<ProjectItem[] | null>(null);
  const [type, setType] = useState("");
  const [status, setStatus] = useState("");
  const [q, setQ] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    listProjects()
      .then((ps) => {
        setProjects(ps);
        if (ps.length > 0) setProjectKey((cur) => cur || ps[0].key);
      })
      .catch((e: Error) => setError(e.message));
  }, []);

  useEffect(() => {
    if (!projectKey) return;
    setItems(null);
    listProjectItems(projectKey)
      .then(setItems)
      .catch((e: Error) => {
        setError(e.message);
        setItems([]);
      });
  }, [projectKey]);

  const shown = useMemo(() => {
    const list = items ?? [];
    const needle = q.trim().toLowerCase();
    return list.filter(
      (i) =>
        (!type || i.item_type === type) &&
        (!status || i.status === status) &&
        (!needle || i.title.toLowerCase().includes(needle) || String(i.number).includes(needle)),
    );
  }, [items, type, status, q]);

  const statuses = useMemo(() => {
    const found = new Set((items ?? []).map((i) => i.status));
    const known = STATUS_OPTIONS.filter((s) => found.has(s));
    const extra = [...found].filter((s) => !STATUS_OPTIONS.includes(s));
    return [...known, ...extra];
  }, [items]);

  return (
    <div className="cp-page">
      <div className="cp-page-head">
        <div>
          <h1 className="cp-page-title">工作项</h1>
          <div className="cp-page-sub">列表视图:按项目 / 类型 / 状态筛选</div>
        </div>
        <div className="cp-page-actions">
          <select value={projectKey} onChange={(e) => setProjectKey(e.target.value)}>
            {projects.map((p) => (
              <option key={p.id} value={p.key}>{p.key} · {p.name}</option>
            ))}
          </select>
        </div>
      </div>

      <div className="cp-toolbar">
        <select value={type} onChange={(e) => setType(e.target.value)}>
          <option value="">全部类型</option>
          {Object.entries(TYPE_LABEL).map(([k, v]) => (
            <option key={k} value={k}>{v}</option>
          ))}
        </select>
        <select value={status} onChange={(e) => setStatus(e.target.value)}>
          <option value="">全部状态</option>
          {statuses.map((s) => (
            <option key={s} value={s}>{statusLabel(s)}</option>
          ))}
        </select>
        <input
          placeholder="搜索标题或编号"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          style={{ width: 220 }}
        />
        {(type || status || q) && (
          <button className="cp-btn cp-btn-sm" onClick={() => { setType(""); setStatus(""); setQ(""); }}>
            清除筛选
          </button>
        )}
        <div className="cp-spacer" />
        <span className="cp-badge cp-badge-neutral">{shown.length} 项</span>
      </div>

      {error && <div className="cp-alert cp-alert-danger">{error}</div>}

      {items === null ? (
        <div className="cp-card cp-card-pad cp-col">
          <div className="cp-skeleton" style={{ height: 40 }} />
          <div className="cp-skeleton" style={{ height: 40 }} />
          <div className="cp-skeleton" style={{ height: 40 }} />
        </div>
      ) : shown.length === 0 ? (
        <div className="cp-card">
          <div className="cp-empty">
            <div className="cp-empty-icon">☰</div>
            <div className="cp-empty-title">没有匹配的工作项</div>
            <div className="cp-empty-hint">试试调整筛选条件</div>
          </div>
        </div>
      ) : (
        <div className="cp-table-wrap">
          <table className="cp-table">
            <thead>
              <tr>
                <th style={{ width: 84 }}>编号</th>
                <th style={{ width: 70 }}>类型</th>
                <th>标题</th>
                <th style={{ width: 96 }}>状态</th>
                <th style={{ width: 84 }}>优先级</th>
                <th style={{ width: 96 }}>经办人</th>
                <th style={{ width: 110 }}>更新时间</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((i) => (
                <tr
                  key={i.id}
                  style={{ cursor: "pointer" }}
                  onClick={() => (window.location.hash = `/item/${i.id}`)}
                >
                  <td className="cp-mono cp-muted">#{i.number}</td>
                  <td><span className="cp-tag">{TYPE_LABEL[i.item_type] ?? i.item_type}</span></td>
                  <td style={{ fontWeight: 500 }}>{i.title}</td>
                  <td><span className={statusBadgeClass(i.status)}>{statusLabel(i.status)}</span></td>
                  <td className="cp-muted">{PRIORITY_LABEL[i.priority] ?? i.priority}</td>
                  <td className="cp-mono cp-muted" style={{ fontSize: 12 }}>
                    {i.assignee ? i.assignee.slice(0, 8) : "—"}
                  </td>
                  <td className="cp-muted" style={{ fontSize: 12 }}>{i.updated_at.slice(0, 10)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
