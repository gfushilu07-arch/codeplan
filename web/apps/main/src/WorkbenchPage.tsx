import { useEffect, useState } from "react";
import { fetchMyItems, type MyItem } from "@codeplan/api-client";

/** 工作台首页:分配给我/我创建聚合(UX-01;命令面板与@我/待办随批次二)。 */
export function WorkbenchPage() {
  const [items, setItems] = useState<MyItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    fetchMyItems()
      .then(setItems)
      .catch((e: Error) => setError(e.message));
  }, []);

  if (error) return <div style={{ color: "var(--cp-color-danger)" }}>{error}</div>;
  if (!items) return <div>加载中…</div>;
  if (items.length === 0) {
    return <div style={{ color: "var(--cp-color-text-muted)" }}>暂无与我相关的工作项</div>;
  }
  return (
    <div>
      <h2 style={{ fontSize: 18, marginTop: 0 }}>我的工作项({items.length})</h2>
      <ul style={{ listStyle: "none", padding: 0, display: "flex", flexDirection: "column", gap: "var(--cp-space-2)" }}>
        {items.map((item) => (
          <li
            key={item.id}
            style={{
              background: "var(--cp-color-bg)",
              border: "1px solid var(--cp-color-border)",
              borderRadius: "var(--cp-radius-md)",
              padding: "var(--cp-space-2) var(--cp-space-3)",
              display: "flex",
              gap: "var(--cp-space-2)",
              alignItems: "center",
            }}
          >
            <span
              style={{
                fontSize: 12,
                padding: "2px 8px",
                borderRadius: "var(--cp-radius-sm)",
                background: "var(--cp-color-surface-alt)",
                fontFamily: "var(--cp-font-mono)",
              }}
            >
              {item.item_type}/{item.status}
            </span>
            <span>{item.title}</span>
            <span style={{ marginLeft: "auto", fontSize: 12, color: "var(--cp-color-text-muted)" }}>
              {item.updated_at.slice(0, 10)}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}
