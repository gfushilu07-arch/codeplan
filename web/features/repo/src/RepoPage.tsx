import { useEffect, useMemo, useState } from "react";
import {
  fetchExternalLinks,
  repoBlob,
  repoCommits,
  repoTree,
  type RepoCommit,
  type RepoTreeEntry,
  type ResolvedLink,
} from "@codeplan/api-client";
import { PullsView } from "./PullsView";
import { ReleasesView } from "./ReleasesView";

type Tab = "files" | "commits" | "pulls" | "releases";

const TABS: Array<{ key: Tab; label: string }> = [
  { key: "files", label: "代码" },
  { key: "commits", label: "提交" },
  { key: "pulls", label: "Pull Requests" },
  { key: "releases", label: "发布" },
];

/**
 * 仓库页(T-134)。布局对齐 gitcode.com:仓库头 + 标签栏(48px/间距 32px)+
 * 分支工具栏(32px)+ 双栏主体(文件列表 | 简介侧栏,1210px 容器内 66/34)。
 */
export function RepoPage({
  owner,
  repoKey,
  defaultBranch,
}: {
  owner: string;
  repoKey: string;
  defaultBranch: string;
}) {
  const [tab, setTab] = useState<Tab>("files");
  const [reference] = useState(defaultBranch);
  const [entries, setEntries] = useState<RepoTreeEntry[]>([]);
  const [commits, setCommits] = useState<RepoCommit[]>([]);
  const [selected, setSelected] = useState<{ path: string; content: string } | null>(null);
  const [readme, setReadme] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [filter, setFilter] = useState("");
  const [links, setLinks] = useState<ResolvedLink[]>([]);

  // 外部链接器(T-308 / MS-03):按仓库 + 当前引用解析模板
  useEffect(() => {
    fetchExternalLinks(owner, repoKey, "", reference)
      .then(setLinks)
      .catch(() => setLinks([]));
  }, [owner, repoKey, reference]);

  useEffect(() => {
    if (tab !== "files") return;
    repoTree(owner, repoKey, reference)
      .then(setEntries)
      .catch((e: Error) => setError(e.message));
  }, [owner, repoKey, reference, tab]);

  useEffect(() => {
    if (tab !== "commits") return;
    repoCommits(owner, repoKey, reference, 50)
      .then(setCommits)
      .catch((e: Error) => setError(e.message));
  }, [owner, repoKey, reference, tab]);

  // 简介侧栏:存在 README 时拉取渲染(对应 gitcode 的 Introduction 面板)
  useEffect(() => {
    const readmeEntry = entries.find((e) => /^readme(\.md|\.markdown|\.txt)?$/i.test(e.name));
    if (!readmeEntry) {
      setReadme(null);
      return;
    }
    repoBlob(owner, repoKey, reference, readmeEntry.name)
      .then(setReadme)
      .catch(() => setReadme(null));
  }, [entries, owner, repoKey, reference]);

  const openBlob = async (path: string) => {
    try {
      const content = await repoBlob(owner, repoKey, reference, path);
      setSelected({ path, content });
    } catch (e) {
      setError((e as Error).message);
    }
  };

  const cloneUrl = `http://localhost:8081/git/${owner}/${repoKey}.git`;
  const copyClone = () => {
    void navigator.clipboard?.writeText(`git clone ${cloneUrl}`).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1600);
    });
  };

  const visible = useMemo(() => {
    const q = filter.trim().toLowerCase();
    return q ? entries.filter((e) => e.name.toLowerCase().includes(q)) : entries;
  }, [entries, filter]);

  const treeCount = entries.filter((e) => e.kind === "tree").length;

  return (
    <div className="cp-page">
      <nav className="cp-breadcrumb">
        <a href="#/repos">代码</a>
        <span className="sep">/</span>
        <a href="#/repos">{owner}</a>
        <span className="sep">/</span>
        <span className="current">{repoKey}</span>
      </nav>

      {/* 仓库头 + 标签栏 */}
      <div>
        <div className="cp-repo-head">
          <h1 className="cp-repo-title">
            <span className="cp-muted" style={{ fontWeight: 400 }}>{owner} /</span>
            {repoKey}
            <span className="cp-badge cp-badge-neutral">私有</span>
          </h1>
          <div className="cp-spacer" />
          {links.map((l) => (
            <a
              key={l.name}
              className="cp-btn cp-btn-sm"
              href={l.url}
              target="_blank"
              rel="noreferrer"
              title={l.url}
            >
              ↗ {l.name}
            </a>
          ))}
          <button className="cp-btn cp-btn-primary" onClick={copyClone}>
            {copied ? "已复制地址" : "克隆 / 下载"}
          </button>
        </div>
        <div className="cp-tabs" style={{ marginTop: 4 }}>
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
      </div>

      {error && <div className="cp-alert cp-alert-danger">{error}</div>}

      {tab === "pulls" && <PullsView owner={owner} repoKey={repoKey} />}
      {tab === "releases" && <ReleasesView owner={owner} repoKey={repoKey} />}

      {tab === "files" && (
        <>
          {/* 分支工具栏 */}
          <div className="cp-repo-toolbar">
            <span className="cp-branch-chip">⎇ {reference}</span>
            <span className="cp-muted" style={{ fontSize: 12 }}>
              {entries.length} 个文件 · {treeCount} 个目录
            </span>
            <div className="cp-spacer" />
            <input
              placeholder="搜索文件"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              style={{ width: 190, height: 32 }}
            />
          </div>

          <div className="cp-repo-layout">
            {/* 主列:文件列表 或 文件内容 */}
            <div className="cp-panel">
              {selected ? (
                <>
                  <div className="cp-panel-head">
                    <button className="cp-btn cp-btn-sm" onClick={() => setSelected(null)}>← 返回</button>
                    <span className="cp-mono">{selected.path}</span>
                  </div>
                  <pre className="cp-code" style={{ borderRadius: 0, border: 0, maxHeight: 620 }}>
                    {selected.content}
                  </pre>
                </>
              ) : (
                <>
                  <div className="cp-panel-head">
                    <span>文件</span>
                    <span className="cp-badge cp-badge-neutral">{visible.length}</span>
                  </div>
                  {visible.length === 0 ? (
                    <div className="cp-empty">
                      <div className="cp-empty-icon">📁</div>
                      <div className="cp-empty-title">{filter ? "无匹配文件" : "空目录"}</div>
                      {filter && <div className="cp-empty-hint">试试其他关键词</div>}
                    </div>
                  ) : (
                    <div>
                      {visible.map((e) => (
                        <div
                          key={e.oid + e.name}
                          className={`cp-file-row${e.kind === "blob" ? " clickable" : ""}`}
                          onClick={() => e.kind === "blob" && void openBlob(e.name)}
                        >
                          <span style={{ fontSize: 15 }}>{e.kind === "tree" ? "📁" : "📄"}</span>
                          <span className="name cp-nowrap">{e.name}</span>
                          {e.size != null && <span className="cp-spacer cp-mono cp-muted">{e.size} B</span>}
                        </div>
                      ))}
                    </div>
                  )}
                </>
              )}
            </div>

            {/* 侧栏:简介(gitcode Introduction 面板) */}
            <aside className="cp-panel">
              <div className="cp-panel-head">
                <span>简介</span>
              </div>
              <div className="cp-panel-body">
                {readme ? (
                  <div style={{ whiteSpace: "pre-wrap", fontSize: 13, lineHeight: 1.7, maxHeight: 420, overflow: "auto" }}>
                    {readme}
                  </div>
                ) : (
                  <div className="cp-muted" style={{ fontSize: 13 }}>
                    暂无 README。在仓库根目录添加 <code className="cp-mono">README.md</code> 后会显示在这里。
                  </div>
                )}
                <div style={{ marginTop: "var(--cp-space-4)", paddingTop: "var(--cp-space-3)", borderTop: "1px solid var(--cp-color-border-light)" }}>
                  <div className="cp-muted" style={{ fontSize: 12, marginBottom: 6 }}>克隆地址</div>
                  <div className="cp-row">
                    <input
                      readOnly
                      value={cloneUrl}
                      className="cp-input-mono"
                      style={{ flex: 1, minWidth: 0 }}
                      onFocus={(e) => e.currentTarget.select()}
                    />
                    <button className="cp-btn cp-btn-sm" onClick={copyClone}>{copied ? "已复制" : "复制"}</button>
                  </div>
                </div>
              </div>
            </aside>
          </div>
        </>
      )}

      {tab === "commits" && (
        <div className="cp-panel">
          <div className="cp-panel-head">
            <span>提交历史</span>
            <span className="cp-badge cp-badge-neutral">{commits.length}</span>
          </div>
          {commits.length === 0 ? (
            <div className="cp-empty">
              <div className="cp-empty-icon">⎇</div>
              <div className="cp-empty-title">暂无提交</div>
            </div>
          ) : (
            <div>
              {commits.map((c) => (
                <div className="cp-file-row" key={c.id} style={{ height: "auto", padding: "10px var(--cp-space-4)" }}>
                  <span className="cp-avatar cp-avatar-sm">{(c.author_name || "?").slice(0, 1).toUpperCase()}</span>
                  <div style={{ minWidth: 0, flex: 1 }}>
                    <div className="cp-nowrap">{c.summary}</div>
                    <div className="cp-list-meta">
                      <span className="cp-mono">{c.id.slice(0, 8)}</span>
                      <span>{c.author_name}</span>
                      <span>{new Date(c.time * 1000).toLocaleString()}</span>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
