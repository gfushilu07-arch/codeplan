import { useCallback, useEffect, useState } from "react";
import {
  addDeployTarget,
  applyAppConfig,
  applyGateway,
  autoAssignDomain,
  applyServiceSpec,
  createApp,
  createAppEnv,
  deleteAppEnv,
  deleteEnvVar,
  bindAppDomain,
  fetchAppAggregate,
  fetchAppConfig,
  fetchGatewayNginx,
  issueCertificate,
  fetchServiceSpec,
  listApps,
  listDeployTargets,
  listAppConfigVersions,
  listAppDomains,
  listCertificates,
  listEnvVars,
  listServiceSpecVersions,
  revealAppConfigKey,
  removeDeployTarget,
  unbindAppDomain,
  upsertEnvVar,
  type AppAggregate,
  type AppInfo,
  type DeployTargetInfo,
  type CertInfo,
  type ConfigVersionInfo,
  type DomainInfo,
  type EnvVarInfo,
  type SpecVersionInfo,
} from "@codeplan/api-client";

const ENV_KIND_LABEL: Record<string, string> = {
  dev: "开发",
  test: "测试",
  staging: "预发",
  prod: "生产",
};

/** 应用中心(T-310 / PAAS-01,02):应用 → 环境 → 部署目标 / 变量。 */
export function AppCenterPage() {
  const [apps, setApps] = useState<AppInfo[] | null>(null);
  const [active, setActive] = useState<string | null>(null);
  const [agg, setAgg] = useState<AppAggregate | null>(null);
  const [targets, setTargets] = useState<Record<string, DeployTargetInfo[]>>({});
  const [vars, setVars] = useState<Record<string, EnvVarInfo[]>>({});
  const [newApp, setNewApp] = useState({ key: "", name: "" });
  const [newEnv, setNewEnv] = useState({ name: "", kind: "dev" });
  const [targetDraft, setTargetDraft] = useState<Record<string, { name: string; host: string }>>({});
  const [varDraft, setVarDraft] = useState<Record<string, { key: string; value: string; secret: boolean }>>({});
  const [specs, setSpecs] = useState<Record<string, SpecVersionInfo | null>>({});
  const [specDraft, setSpecDraft] = useState<Record<string, string>>({});
  const [specHistory, setSpecHistory] = useState<Record<string, SpecVersionInfo[]>>({});
  const [specOpen, setSpecOpen] = useState<Record<string, boolean>>({});
  const [configs, setConfigs] = useState<Record<string, ConfigVersionInfo | null>>({});
  const [configDraft, setConfigDraft] = useState<Record<string, string>>({});
  const [revealed, setRevealed] = useState<Record<string, string>>({});
  const [domains, setDomains] = useState<Record<string, DomainInfo[]>>({});
  const [domainDraft, setDomainDraft] = useState<Record<string, string>>({});
  const [nginxCfg, setNginxCfg] = useState<Record<string, string>>({});
  const [certs, setCerts] = useState<Record<string, CertInfo[]>>({});
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const loadApps = useCallback(async () => {
    try {
      const list = await listApps();
      setApps(list);
      setActive((cur) => cur ?? list[0]?.key ?? null);
    } catch (e) {
      setError((e as Error).message);
    }
  }, []);

  useEffect(() => {
    void loadApps();
  }, [loadApps]);

  const loadAgg = useCallback(async (key: string) => {
    try {
      const a = await fetchAppAggregate(key);
      setAgg(a);
      const tEntries = await Promise.all(
        a.environments.map(async (e) => [e.id, await listDeployTargets(key, e.id).catch(() => [])] as const),
      );
      const vEntries = await Promise.all(
        a.environments.map(async (e) => [e.id, await listEnvVars(key, e.id).catch(() => [])] as const),
      );
      setTargets(Object.fromEntries(tEntries));
      setVars(Object.fromEntries(vEntries));
      const sEntries = await Promise.all(
        a.environments.map(async (e) => [e.id, await fetchServiceSpec(key, e.id).catch(() => null)] as const),
      );
      setSpecs(Object.fromEntries(sEntries));
      const cEntries = await Promise.all(
        a.environments.map(async (e) => [e.id, await fetchAppConfig(key, e.id).catch(() => null)] as const),
      );
      setConfigs(Object.fromEntries(cEntries));
      const dEntries = await Promise.all(
        a.environments.map(async (e) => [e.id, await listAppDomains(key, e.id).catch(() => [])] as const),
      );
      setDomains(Object.fromEntries(dEntries));
      const certEntries = await Promise.all(
        a.environments.map(async (e) => [e.id, await listCertificates(key, e.id).catch(() => [])] as const),
      );
      setCerts(Object.fromEntries(certEntries));
    } catch (e) {
      setError((e as Error).message);
      setAgg(null);
    }
  }, []);

  useEffect(() => {
    if (!active) return;
    setAgg(null);
    setTargets({});
    setVars({});
    void loadAgg(active);
  }, [active, loadAgg]);

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
          <h1 className="cp-page-title">应用中心</h1>
          <div className="cp-page-sub">应用聚合仓库 · 环境与部署目标 · 变量注入</div>
        </div>
        <div className="cp-page-actions">
          <input
            placeholder="应用键(小写-连字符)"
            value={newApp.key}
            onChange={(e) => setNewApp({ ...newApp, key: e.target.value })}
            style={{ width: 170 }}
          />
          <input
            placeholder="应用名"
            value={newApp.name}
            onChange={(e) => setNewApp({ ...newApp, name: e.target.value })}
            style={{ width: 150 }}
          />
          <button
            className="cp-btn-primary"
            disabled={busy || !newApp.key.trim() || !newApp.name.trim()}
            onClick={() =>
              void guard(async () => {
                const created = await createApp(newApp.key.trim(), newApp.name.trim());
                setNewApp({ key: "", name: "" });
                await loadApps();
                setActive(created.key);
              })
            }
          >
            新建应用
          </button>
        </div>
      </div>

      {error && <div className="cp-alert cp-alert-danger">{error}</div>}

      <div style={{ display: "grid", gridTemplateColumns: "260px minmax(0, 1fr)", gap: "var(--cp-space-4)", alignItems: "start" }}>
        <aside className="cp-panel">
          <div className="cp-panel-head">
            应用
            <span className="cp-badge cp-badge-neutral">{apps?.length ?? 0}</span>
          </div>
          <div className="cp-panel-body" style={{ padding: "var(--cp-space-2)" }}>
            <div className="cp-nav-list">
              {apps === null && <div className="cp-muted" style={{ fontSize: 12, padding: 8 }}>加载中…</div>}
              {apps?.length === 0 && <div className="cp-muted" style={{ fontSize: 12, padding: 8 }}>暂无应用</div>}
              {apps?.map((a) => (
                <button
                  key={a.id}
                  className={`cp-nav-list-item${active === a.key ? " active" : ""}`}
                  onClick={() => setActive(a.key)}
                >
                  <span className="cp-nowrap" style={{ flex: 1 }}>{a.name}</span>
                  <span className="cp-mono cp-muted" style={{ fontSize: 11 }}>{a.key}</span>
                </button>
              ))}
            </div>
          </div>
        </aside>

        <div className="cp-col" style={{ gap: "var(--cp-space-4)" }}>
          {!agg ? (
            <div className="cp-panel">
              <div className="cp-empty">
                <div className="cp-empty-icon">🚀</div>
                <div className="cp-empty-title">先创建或选择一个应用</div>
              </div>
            </div>
          ) : (
            <>
              <div className="cp-panel">
                <div className="cp-panel-head">
                  <span>{agg.application.name}</span>
                  <span className="cp-mono cp-muted" style={{ fontSize: 12 }}>{agg.application.key}</span>
                  <div className="cp-spacer" />
                  <input
                    placeholder="新环境名"
                    value={newEnv.name}
                    onChange={(e) => setNewEnv({ ...newEnv, name: e.target.value })}
                    style={{ width: 130 }}
                  />
                  <select value={newEnv.kind} onChange={(e) => setNewEnv({ ...newEnv, kind: e.target.value })}>
                    {Object.entries(ENV_KIND_LABEL).map(([k, v]) => (
                      <option key={k} value={k}>{v}</option>
                    ))}
                  </select>
                  <button
                    className="cp-btn cp-btn-sm"
                    disabled={busy || !newEnv.name.trim()}
                    onClick={() =>
                      void guard(async () => {
                        await createAppEnv(agg.application.key, newEnv.name.trim(), newEnv.kind);
                        setNewEnv({ name: "", kind: "dev" });
                        await loadAgg(agg.application.key);
                      })
                    }
                  >
                    新建环境
                  </button>
                </div>
                <div className="cp-panel-body">
                  {agg.application.description && (
                    <div className="cp-muted" style={{ fontSize: 13, marginBottom: "var(--cp-space-3)" }}>
                      {agg.application.description}
                    </div>
                  )}
                  <div className="cp-row" style={{ flexWrap: "wrap", gap: "var(--cp-space-2)" }}>
                    <span className="cp-muted" style={{ fontSize: 12 }}>关联仓库:</span>
                    {agg.repos.length === 0 ? (
                      <span className="cp-muted" style={{ fontSize: 12 }}>未关联</span>
                    ) : (
                      agg.repos.map((r) => (
                        <span key={r.id} className="cp-tag cp-tag-mono">{r.full_name}</span>
                      ))
                    )}
                  </div>
                </div>
              </div>

              {agg.environments.length === 0 ? (
                <div className="cp-panel">
                  <div className="cp-empty">
                    <div className="cp-empty-icon">🌱</div>
                    <div className="cp-empty-title">还没有环境</div>
                    <div className="cp-empty-hint">至少建一个环境后才能登记部署目标</div>
                  </div>
                </div>
              ) : (
                agg.environments.map((e) => (
                  <div className="cp-panel" key={e.id}>
                    <div className="cp-panel-head">
                      <span>{e.name}</span>
                      <span className={e.production ? "cp-badge cp-badge-danger" : "cp-badge cp-badge-neutral"}>
                        {ENV_KIND_LABEL[e.kind] ?? e.kind}
                      </span>
                      <span className="cp-muted" style={{ fontSize: 12 }}>
                        {e.target_count} 个目标 · {e.var_count} 个变量
                      </span>
                      <div className="cp-spacer" />
                      <button
                        className="cp-btn cp-btn-sm danger"
                        disabled={busy}
                        onClick={() =>
                          void guard(async () => {
                            await deleteAppEnv(agg.application.key, e.id);
                            await loadAgg(agg.application.key);
                          })
                        }
                      >
                        删除环境
                      </button>
                    </div>
                    <div className="cp-panel-body cp-col" style={{ gap: "var(--cp-space-3)" }}>
                      {/* 部署目标 */}
                      <div>
                        <div className="cp-section-title" style={{ marginBottom: 6 }}>部署目标</div>
                        <div className="cp-col" style={{ gap: 6 }}>
                          {(targets[e.id] ?? []).map((t) => (
                            <div className="cp-row" key={t.id} style={{ fontSize: 13 }}>
                              <span>{t.name}</span>
                              <span className="cp-mono cp-muted">{t.host}:{t.port}</span>
                              {t.labels.map((l) => (
                                <span className="cp-tag" key={l}>{l}</span>
                              ))}
                              <div className="cp-spacer" />
                              <button
                                className="cp-btn cp-btn-sm ghost"
                                disabled={busy}
                                onClick={() =>
                                  void guard(async () => {
                                    await removeDeployTarget(agg.application.key, e.id, t.id);
                                    await loadAgg(agg.application.key);
                                  })
                                }
                              >
                                移除
                              </button>
                            </div>
                          ))}
                          {(targets[e.id] ?? []).length === 0 && (
                            <div className="cp-muted" style={{ fontSize: 12 }}>暂无目标</div>
                          )}
                        </div>
                        <div className="cp-row" style={{ marginTop: "var(--cp-space-2)" }}>
                          <input
                            placeholder="目标名"
                            value={targetDraft[e.id]?.name ?? ""}
                            onChange={(ev) =>
                              setTargetDraft((d) => ({ ...d, [e.id]: { name: ev.target.value, host: d[e.id]?.host ?? "" } }))
                            }
                            style={{ width: 120 }}
                          />
                          <input
                            placeholder="主机地址"
                            value={targetDraft[e.id]?.host ?? ""}
                            onChange={(ev) =>
                              setTargetDraft((d) => ({ ...d, [e.id]: { name: d[e.id]?.name ?? "", host: ev.target.value } }))
                            }
                            style={{ flex: 1, minWidth: 140 }}
                          />
                          <button
                            className="cp-btn cp-btn-sm"
                            disabled={busy || !(targetDraft[e.id]?.name ?? "").trim() || !(targetDraft[e.id]?.host ?? "").trim()}
                            onClick={() =>
                              void guard(async () => {
                                await addDeployTarget(agg.application.key, e.id, {
                                  name: (targetDraft[e.id]?.name ?? "").trim(),
                                  host: (targetDraft[e.id]?.host ?? "").trim(),
                                });
                                setTargetDraft((d) => ({ ...d, [e.id]: { name: "", host: "" } }));
                                await loadAgg(agg.application.key);
                              })
                            }
                          >
                            登记
                          </button>
                        </div>
                      </div>

                      {/* 变量 */}
                      <div>
                        <div className="cp-section-title" style={{ marginBottom: 6 }}>环境变量</div>
                        <div className="cp-col" style={{ gap: 6 }}>
                          {(vars[e.id] ?? []).map((v) => (
                            <div className="cp-row" key={v.id} style={{ fontSize: 13 }}>
                              <span className="cp-mono">{v.key}</span>
                              <span className="cp-mono cp-muted">
                                {v.is_secret ? "••••••(敏感)" : v.value}
                              </span>
                              <div className="cp-spacer" />
                              <button
                                className="cp-btn cp-btn-sm ghost"
                                disabled={busy}
                                onClick={() =>
                                  void guard(async () => {
                                    await deleteEnvVar(agg.application.key, e.id, v.id);
                                    await loadAgg(agg.application.key);
                                  })
                                }
                              >
                                删除
                              </button>
                            </div>
                          ))}
                          {(vars[e.id] ?? []).length === 0 && (
                            <div className="cp-muted" style={{ fontSize: 12 }}>暂无变量</div>
                          )}
                        </div>
                        <div className="cp-row" style={{ marginTop: "var(--cp-space-2)" }}>
                          <input
                            placeholder="KEY"
                            className="cp-input-mono"
                            value={varDraft[e.id]?.key ?? ""}
                            onChange={(ev) =>
                              setVarDraft((d) => ({
                                ...d,
                                [e.id]: { key: ev.target.value, value: d[e.id]?.value ?? "", secret: d[e.id]?.secret ?? false },
                              }))
                            }
                            style={{ width: 150 }}
                          />
                          <input
                            placeholder="值"
                            value={varDraft[e.id]?.value ?? ""}
                            onChange={(ev) =>
                              setVarDraft((d) => ({
                                ...d,
                                [e.id]: { key: d[e.id]?.key ?? "", value: ev.target.value, secret: d[e.id]?.secret ?? false },
                              }))
                            }
                            style={{ flex: 1, minWidth: 140 }}
                          />
                          <label className="cp-check">
                            <input
                              type="checkbox"
                              checked={varDraft[e.id]?.secret ?? false}
                              onChange={(ev) =>
                                setVarDraft((d) => ({
                                  ...d,
                                  [e.id]: { key: d[e.id]?.key ?? "", value: d[e.id]?.value ?? "", secret: ev.target.checked },
                                }))
                              }
                            />
                            敏感
                          </label>
                          <button
                            className="cp-btn cp-btn-sm"
                            disabled={busy || !(varDraft[e.id]?.key ?? "").trim()}
                            onClick={() =>
                              void guard(async () => {
                                await upsertEnvVar(agg.application.key, e.id, {
                                  key: (varDraft[e.id]?.key ?? "").trim(),
                                  value: varDraft[e.id]?.value ?? "",
                                  is_secret: varDraft[e.id]?.secret ?? false,
                                });
                                setVarDraft((d) => ({ ...d, [e.id]: { key: "", value: "", secret: false } }));
                                await loadAgg(agg.application.key);
                              })
                            }
                          >
                            写入
                          </button>
                        </div>
                      </div>

                      {/* 服务规格(规格即代码,T-312) */}
                      <div>
                        <div className="cp-row" style={{ marginBottom: 6 }}>
                          <div className="cp-section-title" style={{ margin: 0 }}>服务规格</div>
                          {specs[e.id] ? (
                            <span className="cp-badge cp-badge-info">v{specs[e.id]?.version}</span>
                          ) : (
                            <span className="cp-muted" style={{ fontSize: 12 }}>未提交</span>
                          )}
                          <div className="cp-spacer" />
                          <button
                            className="cp-btn cp-btn-sm"
                            onClick={() =>
                              void guard(async () => {
                                if (!specOpen[e.id]) {
                                  const hist = await listServiceSpecVersions(agg.application.key, e.id).catch(() => []);
                                  setSpecHistory((h) => ({ ...h, [e.id]: hist }));
                                }
                                setSpecOpen((o) => ({ ...o, [e.id]: !o[e.id] }));
                              })
                            }
                          >
                            {specOpen[e.id] ? "收起" : "版本历史"}
                          </button>
                        </div>

                        {specs[e.id] && (
                          <div className="cp-muted" style={{ fontSize: 12, marginBottom: 6 }}>
                            镜像 <span className="cp-mono">{specs[e.id]?.spec.image}</span> · 副本 {specs[e.id]?.spec.replicas} · 端口{" "}
                            {specs[e.id]?.spec.ports.map((p) => p.container).join(",") || "—"}
                          </div>
                        )}

                        {specOpen[e.id] && (
                          <div className="cp-col" style={{ gap: 4, marginBottom: 6 }}>
                            {(specHistory[e.id] ?? []).map((v) => (
                              <div className="cp-row" key={v.version} style={{ fontSize: 12 }}>
                                <span className="cp-badge cp-badge-neutral">v{v.version}</span>
                                <span className="cp-mono">{v.spec.image}</span>
                                <span className="cp-muted">副本 {v.spec.replicas}</span>
                                <div className="cp-spacer" />
                                <span className="cp-muted">{new Date(v.created_at).toLocaleString()}</span>
                              </div>
                            ))}
                            {(specHistory[e.id] ?? []).length === 0 && (
                              <div className="cp-muted" style={{ fontSize: 12 }}>暂无版本</div>
                            )}
                          </div>
                        )}

                        <textarea
                          rows={4}
                          className="cp-input-mono"
                          placeholder={"image: registry.internal/app:1.0.0\nreplicas: 2\nports:\n  - container: 8080"}
                          value={specDraft[e.id] ?? ""}
                          onChange={(ev) => setSpecDraft((d) => ({ ...d, [e.id]: ev.target.value }))}
                          style={{ width: "100%" }}
                        />
                        <div className="cp-row" style={{ marginTop: "var(--cp-space-2)" }}>
                          <button
                            className="cp-btn-primary cp-btn-sm"
                            disabled={busy || !(specDraft[e.id] ?? "").trim()}
                            onClick={() =>
                              void guard(async () => {
                                await applyServiceSpec(agg.application.key, e.id, (specDraft[e.id] ?? "").trim());
                                setSpecDraft((d) => ({ ...d, [e.id]: "" }));
                                await loadAgg(agg.application.key);
                              })
                            }
                          >
                            提交规格
                          </button>
                          <span className="cp-muted" style={{ fontSize: 12 }}>YAML 或 JSON;未知字段会被拒绝</span>
                        </div>
                      </div>

                      {/* 配置与密钥(T-317) */}
                      <div>
                        <div className="cp-row" style={{ marginBottom: 6 }}>
                          <div className="cp-section-title" style={{ margin: 0 }}>配置与密钥</div>
                          {configs[e.id] ? (
                            <span className="cp-badge cp-badge-info">v{configs[e.id]?.version}</span>
                          ) : (
                            <span className="cp-muted" style={{ fontSize: 12 }}>未提交</span>
                          )}
                          <span className="cp-muted" style={{ fontSize: 11 }}>敏感项密文落库</span>
                        </div>

                        {(configs[e.id]?.items ?? []).length > 0 && (
                          <div className="cp-col" style={{ gap: 4, marginBottom: 6 }}>
                            {(configs[e.id]?.items ?? []).map((it) => (
                              <div className="cp-row" key={it.key} style={{ fontSize: 13 }}>
                                <span className="cp-mono" style={{ width: 160 }}>{it.key}</span>
                                <span className="cp-mono cp-muted">
                                  {it.secret ? (revealed[`${e.id}:${it.key}`] ?? "••••••") : it.value}
                                </span>
                                {it.secret && (
                                  <>
                                    <div className="cp-spacer" />
                                    <button
                                      className="cp-btn cp-btn-sm"
                                      disabled={busy}
                                      onClick={() =>
                                        void guard(async () => {
                                          const r = await revealAppConfigKey(agg.application.key, e.id, it.key);
                                          setRevealed((x) => ({ ...x, [`${e.id}:${it.key}`]: r.value }));
                                        })
                                      }
                                    >
                                      {revealed[`${e.id}:${it.key}`] ? "已解密" : "解密"}
                                    </button>
                                  </>
                                )}
                              </div>
                            ))}
                          </div>
                        )}

                        <textarea
                          rows={3}
                          className="cp-input-mono"
                          placeholder={"LOG_LEVEL: info\nDB_PASSWORD:\n  value: s3cret\n  secret: true"}
                          value={configDraft[e.id] ?? ""}
                          onChange={(ev) => setConfigDraft((d) => ({ ...d, [e.id]: ev.target.value }))}
                          style={{ width: "100%" }}
                        />
                        <div className="cp-row" style={{ marginTop: "var(--cp-space-2)" }}>
                          <button
                            className="cp-btn-primary cp-btn-sm"
                            disabled={busy || !(configDraft[e.id] ?? "").trim()}
                            onClick={() =>
                              void guard(async () => {
                                await applyAppConfig(agg.application.key, e.id, (configDraft[e.id] ?? "").trim());
                                setConfigDraft((d) => ({ ...d, [e.id]: "" }));
                                setRevealed({});
                                await loadAgg(agg.application.key);
                              })
                            }
                          >
                            提交配置
                          </button>
                          <button
                            className="cp-btn cp-btn-sm"
                            disabled={busy}
                            onClick={() =>
                              void guard(async () => {
                                const hist = await listAppConfigVersions(agg.application.key, e.id).catch(() => []);
                                setRevealed((x) => ({ ...x, [`${e.id}:hist`]: `${hist.length} 个版本:v${hist.map((h) => h.version).join(", v")}` }));
                              })
                            }
                          >
                            版本数
                          </button>
                          {revealed[`${e.id}:hist`] && (
                            <span className="cp-muted" style={{ fontSize: 12 }}>{revealed[`${e.id}:hist`]}</span>
                          )}
                        </div>
                      </div>

                      {/* 域名绑定与网关(T-315) */}
                      <div>
                        <div className="cp-row" style={{ marginBottom: 6 }}>
                          <div className="cp-section-title" style={{ margin: 0 }}>域名绑定</div>
                          <span className="cp-badge cp-badge-neutral">{(domains[e.id] ?? []).length}</span>
                          <div className="cp-spacer" />
                          <button
                            className="cp-btn cp-btn-sm"
                            disabled={busy}
                            onClick={() =>
                              void guard(async () => {
                                await autoAssignDomain(agg.application.key, e.id);
                                await loadAgg(agg.application.key);
                              })
                            }
                          >
                            自动分配
                          </button>
                          <button
                            className="cp-btn cp-btn-sm"
                            disabled={busy}
                            onClick={() =>
                              void guard(async () => {
                                const cfg = await fetchGatewayNginx(agg.application.key, e.id);
                                setNginxCfg((c) => ({ ...c, [e.id]: c[e.id] ? "" : cfg }));
                              })
                            }
                          >
                            {nginxCfg[e.id] ? "收起配置" : "预览网关"}
                          </button>
                          <button
                            className="cp-btn cp-btn-sm"
                            disabled={busy}
                            onClick={() =>
                              void guard(async () => {
                                const r = await applyGateway(agg.application.key, e.id);
                                setRevealed((x) => ({ ...x, [`${e.id}:gw`]: `已落地 ${r.path}(${r.bytes} 字节)` }));
                              })
                            }
                          >
                            落地网关
                          </button>
                        </div>

                        <div className="cp-col" style={{ gap: 4 }}>
                          {(domains[e.id] ?? []).map((d) => (
                            <div className="cp-row" key={d.id} style={{ fontSize: 13 }}>
                              <span className="cp-mono">{d.host}</span>
                              <span className={d.kind === "auto" ? "cp-badge cp-badge-info" : "cp-badge cp-badge-neutral"}>
                                {d.kind === "auto" ? "自动" : "自定义"}
                              </span>
                              {d.tls_enabled && (
                                <span className={d.cert_ref ? "cp-badge cp-badge-success" : "cp-badge cp-badge-warning"}>
                                  {d.cert_ref ? "TLS 已签发" : "TLS 待签发"}
                                </span>
                              )}
                              <div className="cp-spacer" />
                              <button
                                className="cp-btn cp-btn-sm ghost"
                                disabled={busy}
                                onClick={() =>
                                  void guard(async () => {
                                    await unbindAppDomain(agg.application.key, e.id, d.id);
                                    await loadAgg(agg.application.key);
                                  })
                                }
                              >
                                解绑
                              </button>
                            </div>
                          ))}
                          {(domains[e.id] ?? []).length === 0 && (
                            <div className="cp-muted" style={{ fontSize: 12 }}>尚未绑定域名</div>
                          )}
                        </div>

                        <div className="cp-row" style={{ marginTop: "var(--cp-space-2)" }}>
                          <input
                            placeholder="自定义域名,如 portal.example.com"
                            value={domainDraft[e.id] ?? ""}
                            onChange={(ev) => setDomainDraft((d) => ({ ...d, [e.id]: ev.target.value }))}
                            style={{ flex: 1, minWidth: 200 }}
                          />
                          <button
                            className="cp-btn cp-btn-sm"
                            disabled={busy || !(domainDraft[e.id] ?? "").trim()}
                            onClick={() =>
                              void guard(async () => {
                                await bindAppDomain(agg.application.key, e.id, (domainDraft[e.id] ?? "").trim());
                                setDomainDraft((d) => ({ ...d, [e.id]: "" }));
                                await loadAgg(agg.application.key);
                              })
                            }
                          >
                            绑定
                          </button>
                        </div>

                        <div className="cp-row" style={{ marginTop: "var(--cp-space-2)", flexWrap: "wrap" }}>
                          <span className="cp-muted" style={{ fontSize: 12 }}>证书:</span>
                          {(certs[e.id] ?? []).length === 0 ? (
                            <span className="cp-muted" style={{ fontSize: 12 }}>未签发</span>
                          ) : (
                            (certs[e.id] ?? []).map((c) => (
                              <span key={c.id} className="cp-tag cp-tag-mono">
                                {c.cert_ref} · {c.provider} ·{" "}
                                {c.not_after ? `至 ${c.not_after.slice(0, 10)}` : "有效期未知"}
                              </span>
                            ))
                          )}
                          <button
                            className="cp-btn cp-btn-sm"
                            disabled={busy || (domains[e.id] ?? []).filter((d) => d.tls_enabled).length === 0}
                            onClick={() =>
                              void guard(async () => {
                                const c = await issueCertificate(agg.application.key, e.id);
                                setRevealed((x) => ({
                                  ...x,
                                  [`${e.id}:gw`]: `证书已签发:${c.cert_ref}(${c.provider})`,
                                }));
                                await loadAgg(agg.application.key);
                              })
                            }
                          >
                            申请证书
                          </button>
                        </div>

                        {revealed[`${e.id}:gw`] && (
                          <div className="cp-muted" style={{ fontSize: 12, marginTop: 6 }}>{revealed[`${e.id}:gw`]}</div>
                        )}
                        {nginxCfg[e.id] && (
                          <pre className="cp-code" style={{ marginTop: 6, maxHeight: 260 }}>{nginxCfg[e.id]}</pre>
                        )}
                      </div>
                    </div>
                  </div>
                ))
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
