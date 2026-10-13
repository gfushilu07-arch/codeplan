import { useState } from "react";
import { AdminUsersPage } from "./AdminUsersPage";
import { AdminOrgsPage } from "./AdminOrgsPage";
import { AdminAuditPage } from "./AdminAuditPage";

type Tab = "users" | "orgs" | "audit";

const TABS: Array<{ key: Tab; label: string }> = [
  { key: "users", label: "用户" },
  { key: "orgs", label: "组织" },
  { key: "audit", label: "审计日志" },
];

/**
 * 平台管理页(T-135):用户 / 组织 / 审计日志 三标签(仅平台管理员可见,入口由外壳控制)。
 */
export function AdminPage() {
  const [tab, setTab] = useState<Tab>("users");
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--cp-space-4)" }}>
      <div className="cp-tabs">
        {TABS.map((t) => (
          <button
            key={t.key}
            className={`cp-tab${tab === t.key ? " active" : ""}`}
            onClick={() => setTab(t.key)}
          >
            {t.label}
          </button>
        ))}
      </div>
      {tab === "users" && <AdminUsersPage />}
      {tab === "orgs" && <AdminOrgsPage />}
      {tab === "audit" && <AdminAuditPage />}
    </div>
  );
}
