import { useEffect, useMemo, useState } from "react";
import {
  fetchMyItems,
  listProjects,
  type MyItem,
} from "@codeplan/api-client";
import { statusColor } from "./status";

const DAY_MS = 24 * 3600 * 1000;

/** 甘特图(T-215):近 30 天工作项时间条(created→updated 的 v1 语义)。 */
export function GanttView() {
  const [items, setItems] = useState<MyItem[] | null>(null);
  const [projectKey, setProjectKey] = useState<string>("RPT");
  const [projects, setProjects] = useState<Array<{ id: string; key: string }>>([]);
  const [loadErr, setLoadErr] = useState<string | null>(null);

  useEffect(() => {
    listProjects().then((ps) => {
      setProjects(ps.map((p) => ({ id: p.id, key: (p as unknown as { key: string }).key })));
    }).catch(() => setProjects([]));
  }, []);

  useEffect(() => {
    setItems(null);
    fetchMyItems()
      .then((rows) => setItems([...rows].sort((a, b) => a.updated_at.localeCompare(b.updated_at))))
      .catch((e) => {
        setLoadErr(String(e));
        setItems([]);
      });
  }, [projectKey]);

  const range = useMemo(() => {
    const end = Date.now() + 2 * DAY_MS;
    const start = end - 30 * DAY_MS;
    return { start, end, days: 30 };
  }, []);

  const dayLabels = useMemo(
    () =>
      Array.from({ length: range.days }, (_, i) => {
        const d = new Date(range.start + i * DAY_MS);
        return d.getDate() === 1 || i % 5 === 0 ? `${d.getMonth() + 1}/${d.getDate()}` : "";
      }),
    [range],
  );

  if (loadErr) return <div className="cp-alert cp-alert-danger">加载失败:{loadErr}</div>;

  return (
    <div className="cp-page">
      <div className="cp-page-head">
        <div>
          <h1 className="cp-page-title">甘特图</h1>
          <div className="cp-page-sub">近 30 天工作项时间分布</div>
        </div>
        <div className="cp-page-actions">
          <select value={projectKey} onChange={(e) => setProjectKey(e.target.value)}>
            {projects.map((p) => (
              <option key={p.key} value={p.key}>{p.key}</option>
            ))}
          </select>
        </div>
      </div>

      <div className="cp-card">
        {items === null ? (
          <div className="cp-card-body cp-col">
            <div className="cp-skeleton" style={{ height: 28 }} />
            <div className="cp-skeleton" style={{ height: 28 }} />
            <div className="cp-skeleton" style={{ height: 28 }} />
          </div>
        ) : items.length === 0 ? (
          <div className="cp-empty">
            <div className="cp-empty-icon">📊</div>
            <div className="cp-empty-title">暂无工作项</div>
          </div>
        ) : (
          <div className="cp-card-body">
            <div style={{ display: "flex", marginLeft: 220, fontSize: 11, color: "var(--cp-color-text-muted)", marginBottom: 8 }}>
              {dayLabels.map((label, i) => (
                <div key={i} style={{ width: `${100 / range.days}%`, whiteSpace: "nowrap" }}>{label}</div>
              ))}
            </div>
            {items.map((item) => {
              const updated = new Date(item.updated_at).getTime();
              const created = Math.max(updated - 3 * DAY_MS, range.start);
              const clampedStart = Math.max(created, range.start);
              const clampedEnd = Math.min(updated, range.end);
              if (clampedEnd <= range.start || clampedStart >= range.end) return null;
              const left = ((clampedStart - range.start) / (range.end - range.start)) * 100;
              const width = Math.max(((clampedEnd - clampedStart) / (range.end - range.start)) * 100, 1);
              return (
                <div key={item.id} style={{ display: "flex", alignItems: "center", marginBottom: 6 }}>
                  <div
                    style={{
                      width: 220, flexShrink: 0, fontSize: 12, overflow: "hidden",
                      textOverflow: "ellipsis", whiteSpace: "nowrap", color: "var(--cp-color-text-secondary)",
                      paddingRight: 12,
                    }}
                    title={`${item.item_type}/${item.status} · ${item.title}`}
                  >
                    {item.title}
                  </div>
                  <div style={{ flex: 1, height: 16, background: "var(--cp-color-surface-alt)", borderRadius: "var(--cp-radius-sm)", position: "relative" }}>
                    <div
                      title={`${new Date(updated - 3 * DAY_MS).toLocaleDateString()} → ${new Date(item.updated_at).toLocaleDateString()}`}
                      style={{
                        position: "absolute", left: `${left}%`, width: `${width}%`, top: 3, height: 10,
                        borderRadius: "var(--cp-radius-full)", background: statusColor(item.status),
                      }}
                    />
                  </div>
                </div>
              );
            })}
            <div className="cp-row" style={{ fontSize: 12, color: "var(--cp-color-text-muted)", marginTop: 12, gap: "var(--cp-space-4)" }}>
              <span>图例:</span>
              <span style={{ color: "var(--cp-color-success)" }}>■ 已完成</span>
              <span style={{ color: "var(--cp-color-primary)" }}>■ 进行中</span>
              <span style={{ color: "var(--cp-color-text-muted)" }}>■ 待办</span>
              <span style={{ color: "var(--cp-color-danger)" }}>■ 阻塞</span>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
