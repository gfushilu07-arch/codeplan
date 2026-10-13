import { useCallback, useEffect, useState } from "react";
import { listAudit, type AuditRecord } from "@codeplan/api-client";

const RESOURCE_LABEL: Record<string, string> = {
  system: "系统",
  project: "项目",
  repo: "仓库",
  app: "应用",
};

function actionBadge(action: string): string {
  if (/delete|revoke|disable|reject/.test(action)) return "cp-badge cp-badge-danger";
  if (/create|grant|enable|approve/.test(action)) return "cp-badge cp-badge-success";
  if (/login|logout/.test(action)) return "cp-badge cp-badge-info";
  return "cp-badge cp-badge-neutral";
}

/**
 * 审计日志(ORG-05,T-135):登录、权限变更、仓库/工作项操作全量审计。
 */
export function AdminAuditPage() {
  const [records, setRecords] = useState<AuditRecord[] | null>(null);
  const [limit, setLimit] = useState(100);
  const [q, setQ] = useState("");
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setRecords(await listAudit(limit));
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    }
  }, [limit]);

  useEffect(() => {
    void load();
  }, [load]);

  const shown = (records ?? []).filter((r) => {
    const needle = q.trim().toLowerCase();
    if (!needle) return true;
    return (
      r.action.toLowerCase().includes(needle) ||
      (r.actor ?? "").toLowerCase().includes(needle) ||
      (r.resource_kind ?? "").toLowerCase().includes(needle)
    );
  });

  return (
    <div className="cp-page">
      <div className="cp-page-head">
        <div>
          <h1 className="cp-page-title">审计日志</h1>
          <div className="cp-page-sub">登录、权限变更与业务操作留痕</div>
        </div>
        <div className="cp-page-actions">
          <input
            placeholder="按动作 / 操作者筛选"
            value={q}
            onChange={(e) => setQ(e.target.value)}
            style={{ width: 220 }}
          />
          <select value={limit} onChange={(e) => setLimit(Number(e.target.value))}>
            <option value={100}>最近 100 条</option>
            <option value={200}>最近 200 条</option>
            <option value={500}>最近 500 条</option>
          </select>
          <button className="cp-btn" onClick={() => void load()}>刷新</button>
          <span className="cp-badge cp-badge-neutral">{shown.length} 条</span>
        </div>
      </div>

      {error && <div className="cp-alert cp-alert-danger">{error}</div>}

      {records === null ? (
        <div className="cp-card cp-card-pad cp-col">
          <div className="cp-skeleton" style={{ height: 36 }} />
          <div className="cp-skeleton" style={{ height: 36 }} />
          <div className="cp-skeleton" style={{ height: 36 }} />
        </div>
      ) : shown.length === 0 ? (
        <div className="cp-card">
          <div className="cp-empty">
            <div className="cp-empty-icon">🧾</div>
            <div className="cp-empty-title">暂无审计记录</div>
            <div className="cp-empty-hint">{q ? "试试其他关键词" : "有操作发生时会记录在此"}</div>
          </div>
        </div>
      ) : (
        <div className="cp-table-wrap">
          <table className="cp-table">
            <thead>
              <tr>
                <th style={{ width: 170 }}>时间</th>
                <th style={{ width: 140 }}>操作者</th>
                <th style={{ width: 190 }}>动作</th>
                <th style={{ width: 110 }}>资源</th>
                <th>详情</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((r) => (
                <tr key={r.id}>
                  <td className="cp-muted" style={{ fontSize: 12 }}>
                    {new Date(r.at).toLocaleString()}
                  </td>
                  <td className="cp-mono cp-muted" style={{ fontSize: 12 }}>
                    {r.actor ? r.actor.slice(0, 8) : "系统"}
                  </td>
                  <td>
                    <span className={actionBadge(r.action)}>{r.action}</span>
                  </td>
                  <td className="cp-muted" style={{ fontSize: 12 }}>
                    {r.resource_kind ? (RESOURCE_LABEL[r.resource_kind] ?? r.resource_kind) : "—"}
                    {r.resource_id && <span className="cp-mono"> {r.resource_id.slice(0, 6)}</span>}
                  </td>
                  <td className="cp-mono cp-muted" style={{ fontSize: 11, maxWidth: 380, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                    {r.detail && Object.keys(r.detail).length > 0 ? JSON.stringify(r.detail) : "—"}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
