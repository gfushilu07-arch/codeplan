import { useCallback, useEffect, useState } from "react";
import {
  adminListUsers,
  adminSetUserStatus,
  type AdminUser,
} from "@codeplan/api-client";

/** 平台用户管理页(ORG-01;仅平台管理员可见)。 */
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

  const active = users?.filter((u) => u.status === "active").length ?? 0;

  return (
    <div className="cp-page">
      <div className="cp-page-head">
        <div>
          <h1 className="cp-page-title">用户管理</h1>
          <div className="cp-page-sub">平台成员与账号状态</div>
        </div>
        <div className="cp-page-actions">
          <span className="cp-badge cp-badge-neutral">共 {users?.length ?? 0} 人</span>
          <span className="cp-badge cp-badge-success">正常 {active}</span>
        </div>
      </div>

      {error && <div className="cp-alert cp-alert-danger">{error}</div>}

      {!users ? (
        <div className="cp-card cp-card-pad cp-col">
          <div className="cp-skeleton" style={{ height: 40 }} />
          <div className="cp-skeleton" style={{ height: 40 }} />
        </div>
      ) : (
        <div className="cp-table-wrap">
          <table className="cp-table">
            <thead>
              <tr>
                <th>用户名</th>
                <th>显示名</th>
                <th>邮箱</th>
                <th style={{ width: 100 }}>来源</th>
                <th style={{ width: 100 }}>状态</th>
                <th style={{ width: 110 }}>操作</th>
              </tr>
            </thead>
            <tbody>
              {users.map((u) => (
                <tr key={u.id}>
                  <td>
                    <div className="cp-row">
                      <span className="cp-avatar cp-avatar-sm">{(u.display_name || u.username).slice(0, 1).toUpperCase()}</span>
                      <span style={{ fontWeight: 500 }}>{u.username}</span>
                      {u.is_platform_admin && <span className="cp-badge cp-badge-info">平台管理员</span>}
                    </div>
                  </td>
                  <td>{u.display_name}</td>
                  <td className="cp-muted">{u.email || "—"}</td>
                  <td><span className="cp-tag">{u.source}</span></td>
                  <td>
                    <span className={u.status === "active" ? "cp-badge cp-badge-success" : "cp-badge cp-badge-danger"}>
                      {u.status === "active" ? "正常" : "已禁用"}
                    </span>
                  </td>
                  <td>
                    {u.username === "admin" ? (
                      <span className="cp-muted" style={{ fontSize: 12 }}>内置账号</span>
                    ) : (
                      <button
                        className={`cp-btn cp-btn-sm${u.status === "active" ? " danger" : ""}`}
                        disabled={busyId === u.id}
                        onClick={() => void toggle(u)}
                      >
                        {u.status === "active" ? "禁用" : "启用"}
                      </button>
                    )}
                  </td>
                </tr>
              ))}
              {users.length === 0 && (
                <tr>
                  <td colSpan={6}>
                    <div className="cp-empty">
                      <div className="cp-empty-icon">👤</div>
                      <div className="cp-empty-title">暂无用户</div>
                    </div>
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
