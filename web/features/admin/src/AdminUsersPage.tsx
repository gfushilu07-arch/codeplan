import { useCallback, useEffect, useState } from "react";
import {
  adminListUsers,
  adminSetUserStatus,
  type AdminUser,
} from "@codeplan/api-client";

/** 平台用户管理页(ORG-01;仅平台管理员可见,入口由外壳按身份控制)。 */
export function AdminUsersPage() {
  const [users, setUsers] = useState<AdminUser[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

  const load = useCallback(() => {
    adminListUsers()
      .then(setUsers)
      .catch((e: Error) => setError(e.message));
  }, []);

  useEffect(() => load(), [load]);

  const toggle = async (u: AdminUser) => {
    const target = u.status === "active" ? "disabled" : "active";
    setBusyId(u.id);
    setError(null);
    try {
      await adminSetUserStatus(u.id, target);
      load();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusyId(null);
    }
  };

  if (error) return <div style={{ color: "var(--cp-color-danger)", fontSize: 13 }}>{error}</div>;
  if (!users) return <div style={{ color: "var(--cp-color-text-muted)" }}>加载中…</div>;

  return (
    <div>
      <h2 style={{ fontSize: 18, marginTop: 0 }}>用户管理({users.length})</h2>
      <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 13 }}>
        <thead>
          <tr style={{ textAlign: "left", color: "var(--cp-color-text-muted)" }}>
            <th style={thStyle}>用户名</th>
            <th style={thStyle}>显示名</th>
            <th style={thStyle}>邮箱</th>
            <th style={thStyle}>来源</th>
            <th style={thStyle}>状态</th>
            <th style={thStyle}>操作</th>
          </tr>
        </thead>
        <tbody>
          {users.map((u) => (
            <tr key={u.id} style={{ borderTop: "1px solid var(--cp-color-border)" }}>
              <td style={tdStyle}>
                {u.username}
                {u.is_platform_admin && (
                  <span style={{ marginLeft: 6, fontSize: 11, color: "var(--cp-color-primary)" }}>
                    平台管理员
                  </span>
                )}
              </td>
              <td style={tdStyle}>{u.display_name}</td>
              <td style={tdStyle}>{u.email}</td>
              <td style={tdStyle}>{u.source}</td>
              <td style={tdStyle}>
                <span style={{ color: u.status === "active" ? "var(--cp-color-success)" : "var(--cp-color-danger)" }}>
                  {u.status === "active" ? "正常" : "已禁用"}
                </span>
              </td>
              <td style={tdStyle}>
                {u.username === "admin" ? (
                  <span style={{ color: "var(--cp-color-text-muted)", fontSize: 12 }}>内置账号</span>
                ) : (
                  <button
                    disabled={busyId === u.id}
                    onClick={() => toggle(u)}
                    style={{
                      font: "inherit",
                      fontSize: 12,
                      cursor: "pointer",
                      border: "1px solid var(--cp-color-border)",
                      borderRadius: "var(--cp-radius-sm)",
                      padding: "2px 8px",
                      background: "var(--cp-color-bg)",
                      color: "var(--cp-color-text)",
                    }}
                  >
                    {u.status === "active" ? "禁用" : "启用"}
                  </button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

const thStyle: React.CSSProperties = {
  padding: "var(--cp-space-2)",
  fontWeight: 600,
  fontSize: 12,
};
const tdStyle: React.CSSProperties = {
  padding: "var(--cp-space-2)",
};
