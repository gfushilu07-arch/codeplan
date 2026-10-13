import { useCallback, useEffect, useState } from "react";
import {
  addTeamMember,
  createOrg,
  createTeam,
  listOrgs,
  listTeamMembers,
  listTeams,
  removeTeamMember,
  type OrgInfo,
  type TeamInfo,
  type TeamMemberInfo,
} from "@codeplan/api-client";

/**
 * 组织架构管理(ORG-01,T-135):组织 → 团队 → 成员 三级维护。
 */
export function AdminOrgsPage() {
  const [orgs, setOrgs] = useState<OrgInfo[] | null>(null);
  const [activeOrg, setActiveOrg] = useState<string | null>(null);
  const [teams, setTeams] = useState<TeamInfo[]>([]);
  const [members, setMembers] = useState<Record<string, TeamMemberInfo[]>>({});
  const [newOrg, setNewOrg] = useState("");
  const [newTeam, setNewTeam] = useState("");
  const [memberDraft, setMemberDraft] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const loadOrgs = useCallback(async () => {
    try {
      const list = await listOrgs();
      setOrgs(list);
      setActiveOrg((cur) => cur ?? list[0]?.id ?? null);
    } catch (e) {
      setError((e as Error).message);
    }
  }, []);

  useEffect(() => {
    void loadOrgs();
  }, [loadOrgs]);

  const loadTeams = useCallback(async (orgId: string) => {
    try {
      const list = await listTeams(orgId);
      setTeams(list);
      const entries = await Promise.all(
        list.map(async (t) => [t.id, await listTeamMembers(t.id).catch(() => [])] as const),
      );
      setMembers(Object.fromEntries(entries));
    } catch (e) {
      setError((e as Error).message);
    }
  }, []);

  useEffect(() => {
    if (!activeOrg) return;
    setTeams([]);
    setMembers({});
    void loadTeams(activeOrg);
  }, [activeOrg, loadTeams]);

  const guard = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="cp-page">
      <div className="cp-page-head">
        <div>
          <h1 className="cp-page-title">组织架构</h1>
          <div className="cp-page-sub">组织 → 团队 → 成员</div>
        </div>
        <div className="cp-page-actions">
          <span className="cp-badge cp-badge-neutral">{orgs?.length ?? 0} 个组织</span>
        </div>
      </div>

      {error && <div className="cp-alert cp-alert-danger">{error}</div>}

      <div style={{ display: "grid", gridTemplateColumns: "260px minmax(0, 1fr)", gap: "var(--cp-space-4)", alignItems: "start" }}>
        {/* 组织列 */}
        <aside className="cp-panel">
          <div className="cp-panel-head">组织</div>
          <div className="cp-panel-body" style={{ padding: "var(--cp-space-2)" }}>
            <div className="cp-row" style={{ marginBottom: "var(--cp-space-2)" }}>
              <input
                placeholder="新组织名"
                value={newOrg}
                onChange={(e) => setNewOrg(e.target.value)}
                style={{ flex: 1, minWidth: 0 }}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && newOrg.trim()) {
                    void guard(async () => {
                      await createOrg(newOrg.trim());
                      setNewOrg("");
                      await loadOrgs();
                    });
                  }
                }}
              />
              <button
                className="cp-btn-primary cp-btn-sm"
                disabled={busy || !newOrg.trim()}
                onClick={() =>
                  void guard(async () => {
                    await createOrg(newOrg.trim());
                    setNewOrg("");
                    await loadOrgs();
                  })
                }
              >
                新建
              </button>
            </div>
            <div className="cp-nav-list">
              {orgs === null && <div className="cp-muted" style={{ fontSize: 12, padding: 8 }}>加载中…</div>}
              {orgs?.length === 0 && <div className="cp-muted" style={{ fontSize: 12, padding: 8 }}>暂无组织</div>}
              {orgs?.map((o) => (
                <button
                  key={o.id}
                  className={`cp-nav-list-item${activeOrg === o.id ? " active" : ""}`}
                  onClick={() => setActiveOrg(o.id)}
                >
                  <span className="cp-nowrap" style={{ flex: 1 }}>{o.name}</span>
                  <span className="cp-badge cp-badge-neutral">{o.team_count}</span>
                </button>
              ))}
            </div>
          </div>
        </aside>

        {/* 团队 + 成员 */}
        <div className="cp-col" style={{ gap: "var(--cp-space-4)" }}>
          {!activeOrg ? (
            <div className="cp-panel">
              <div className="cp-empty">
                <div className="cp-empty-icon">🏢</div>
                <div className="cp-empty-title">先创建或选择一个组织</div>
              </div>
            </div>
          ) : (
            <>
              <div className="cp-panel">
                <div className="cp-panel-head">
                  <span>团队</span>
                  <div className="cp-spacer" />
                  <input
                    placeholder="新团队名"
                    value={newTeam}
                    onChange={(e) => setNewTeam(e.target.value)}
                    style={{ width: 180 }}
                  />
                  <button
                    className="cp-btn-primary cp-btn-sm"
                    disabled={busy || !newTeam.trim()}
                    onClick={() =>
                      void guard(async () => {
                        await createTeam(activeOrg, newTeam.trim());
                        setNewTeam("");
                        await loadTeams(activeOrg);
                        await loadOrgs();
                      })
                    }
                  >
                    新建团队
                  </button>
                </div>
                <div className="cp-panel-body cp-col" style={{ gap: "var(--cp-space-3)" }}>
                  {teams.length === 0 && <div className="cp-muted" style={{ fontSize: 13 }}>该组织暂无团队</div>}
                  {teams.map((t) => (
                    <div className="cp-panel" key={t.id} style={{ boxShadow: "none" }}>
                      <div className="cp-panel-head">
                        <span>{t.name}</span>
                        <span className="cp-badge cp-badge-neutral">
                          {(members[t.id] ?? []).length} 名成员
                        </span>
                      </div>
                      <div className="cp-panel-body" style={{ padding: "var(--cp-space-3)" }}>
                        <div className="cp-col" style={{ gap: 6 }}>
                          {(members[t.id] ?? []).map((m) => (
                            <div className="cp-row" key={m.user_id} style={{ fontSize: 13 }}>
                              <span className="cp-avatar cp-avatar-sm">
                                {(m.display_name || m.username || "?").slice(0, 1).toUpperCase()}
                              </span>
                              <span>{m.display_name || m.username}</span>
                              <span className="cp-muted cp-mono" style={{ fontSize: 12 }}>{m.username}</span>
                              <div className="cp-spacer" />
                              <button
                                className="cp-btn cp-btn-sm ghost"
                                disabled={busy}
                                onClick={() =>
                                  void guard(async () => {
                                    await removeTeamMember(t.id, m.user_id);
                                    await loadTeams(activeOrg);
                                  })
                                }
                              >
                                移出
                              </button>
                            </div>
                          ))}
                          {(members[t.id] ?? []).length === 0 && (
                            <div className="cp-muted" style={{ fontSize: 12 }}>暂无成员</div>
                          )}
                        </div>
                        <div className="cp-row" style={{ marginTop: "var(--cp-space-3)" }}>
                          <input
                            placeholder="按登录名添加成员"
                            value={memberDraft[t.id] ?? ""}
                            onChange={(e) => setMemberDraft((d) => ({ ...d, [t.id]: e.target.value }))}
                            style={{ flex: 1, minWidth: 0 }}
                          />
                          <button
                            className="cp-btn cp-btn-sm"
                            disabled={busy || !(memberDraft[t.id] ?? "").trim()}
                            onClick={() =>
                              void guard(async () => {
                                await addTeamMember(t.id, (memberDraft[t.id] ?? "").trim());
                                setMemberDraft((d) => ({ ...d, [t.id]: "" }));
                                await loadTeams(activeOrg);
                              })
                            }
                          >
                            添加
                          </button>
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
