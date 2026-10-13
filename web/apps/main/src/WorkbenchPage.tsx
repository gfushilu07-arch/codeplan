import { useEffect, useMemo, useState } from "react";
import { fetchMyItems, listProjects, type MyItem, type ProjectSummary } from "@codeplan/api-client";
import { statusLabel, statusBadgeClass } from "@codeplan/feature-workitem";

const DONE = new Set(["done", "closed", "resolved"]);

const SIDE_MAIN: Array<{ label: string; icon: string; path: string }> = [
  { label: "工作台", icon: "⌂", path: "/" },
  { label: "项目看板", icon: "▦", path: "/projects" },
  { label: "工作项", icon: "☰", path: "/items" },
  { label: "需求池", icon: "⑃", path: "/backlog" },
  { label: "甘特图", icon: "▤", path: "/gantt" },
];
const SIDE_DEV: Array<{ label: string; icon: string; path: string }> = [
  { label: "代码仓库", icon: "⎇", path: "/repos" },
  { label: "Wiki", icon: "📖", path: "/wiki" },
  { label: "效能报表", icon: "📊", path: "/report" },
  { label: "应用中心", icon: "🚀", path: "/apps" },
];

/** 工作台:按 gitcode 首页布局(左频道栏 + 横幅 + 筛选胶囊 + 双列卡片网格)。 */
export function WorkbenchPage() {
  const [projects, setProjects] = useState<ProjectSummary[] | null>(null);
  const [items, setItems] = useState<MyItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [chip, setChip] = useState<"all" | "recent">("all");

  useEffect(() => {
    listProjects()
      .then(setProjects)
      .catch((e: Error) => setError(e.message));
    fetchMyItems()
      .then(setItems)
      .catch((e: Error) => setError(e.message));
  }, []);

  const stats = useMemo(() => {
    const all = items ?? [];
    const open = all.filter((i) => !DONE.has(i.status));
    return { total: all.length, open: open.length, done: all.length - open.length };
  }, [items]);

  const shown = useMemo(() => {
    const list = projects ?? [];
    if (chip === "recent") {
      return [...list].sort((a, b) => b.created_at.localeCompare(a.created_at)).slice(0, 6);
    }
    return list;
  }, [projects, chip]);

  return (
    <div className="cp-home">
      {/* 左频道栏 */}
      <aside className="cp-home-side">
        {SIDE_MAIN.map((s) => (
          <a key={s.path} href={`#${s.path}`} className={`cp-side-item${s.path === "/" ? " active" : ""}`}>
            <span className="ico">{s.icon}</span>
            <span>{s.label}</span>
          </a>
        ))}
        <div className="cp-side-group">研发</div>
        {SIDE_DEV.map((s) => (
          <a key={s.path} href={`#${s.path}`} className="cp-side-item">
            <span className="ico">{s.icon}</span>
            <span>{s.label}</span>
          </a>
        ))}
        <div className="cp-side-group">概览</div>
        <div className="cp-side-item" style={{ cursor: "default" }}>
          <span className="ico">◦</span>
          <span>待处理 {items ? stats.open : "—"}</span>
        </div>
        <div className="cp-side-item" style={{ cursor: "default" }}>
          <span className="ico">◦</span>
          <span>已完结 {items ? stats.done : "—"}</span>
        </div>
      </aside>

      {/* 主区 */}
      <div className="cp-home-main">
        <div className="cp-home-hero">
          <span className="cp-hero-pill">
            <span className="lead">CodePlan V1</span>
            <span className="tail">从需求到上线的研发闭环</span>
          </span>
        </div>

        <div className="cp-home-chips">
          <button className={`cp-chip${chip === "all" ? " active" : ""}`} onClick={() => setChip("all")}>
            全部项目
          </button>
          <button className={`cp-chip${chip === "recent" ? " active" : ""}`} onClick={() => setChip("recent")}>
            最近创建
          </button>
        </div>

        {error && <div className="cp-alert cp-alert-danger" style={{ marginTop: 16 }}>{error}</div>}

        <div className="cp-home-sec">
          <h2>项目</h2>
          <span className="note">{shown.length} 个</span>
        </div>

        {projects === null ? (
          <div className="cp-home-grid">
            <div className="cp-skeleton" style={{ height: 151 }} />
            <div className="cp-skeleton" style={{ height: 151 }} />
          </div>
        ) : shown.length === 0 ? (
          <div className="cp-panel">
            <div className="cp-empty">
              <div className="cp-empty-icon">📦</div>
              <div className="cp-empty-title">暂无项目</div>
              <div className="cp-empty-hint">创建项目后,它会出现在这里</div>
            </div>
          </div>
        ) : (
          <div className="cp-home-grid">
            {shown.map((p, i) => (
              <article className="cp-repo-card" key={p.id}>
                <div className="cp-repo-card-head">
                  <span className={`cp-rank${i === 0 ? "" : i === 1 ? " cp-rank-2" : i === 2 ? " cp-rank-3" : " cp-rank-plain"}`}>
                    {i < 3 ? `TOP ${i + 1}` : "精选"}
                  </span>
                  <span className="cp-repo-card-name cp-nowrap">{p.name}</span>
                  <a className="cp-card-action" href="#/projects">打开</a>
                </div>
                <div className="cp-repo-card-desc">{p.description || "暂无项目描述。"}</div>
                <div className="cp-repo-card-foot">
                  <span className="cp-lang">
                    <span className="cp-lang-dot" />
                    {p.key}
                  </span>
                  <span className="cp-foot-sep">·</span>
                  <span>创建于 {p.created_at.slice(0, 10)}</span>
                </div>
              </article>
            ))}
          </div>
        )}

        <div className="cp-home-sec">
          <h2>我的工作项</h2>
          <span className="note">{items ? `${stats.open} 项待处理` : "加载中…"}</span>
        </div>

        <div className="cp-panel">
          {items === null ? (
            <div className="cp-card-body cp-col">
              <div className="cp-skeleton" style={{ height: 44 }} />
              <div className="cp-skeleton" style={{ height: 44 }} />
            </div>
          ) : items.length === 0 ? (
            <div className="cp-empty">
              <div className="cp-empty-icon">☰</div>
              <div className="cp-empty-title">暂无与我相关的工作项</div>
            </div>
          ) : (
            <div className="cp-list">
              {items.slice(0, 8).map((item) => (
                <div className="cp-list-item" key={item.id}>
                  <span className="cp-avatar cp-avatar-sm">{(item.item_type || "?").slice(0, 1).toUpperCase()}</span>
                  <div className="cp-list-main">
                    <div className="cp-list-title cp-nowrap">{item.title}</div>
                    <div className="cp-list-meta">
                      <span className="cp-tag cp-tag-mono">{item.item_type}</span>
                      <span>更新于 {item.updated_at.slice(0, 10)}</span>
                    </div>
                  </div>
                  <span className={statusBadgeClass(item.status)}>{statusLabel(item.status)}</span>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
