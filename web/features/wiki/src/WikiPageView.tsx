import { useCallback, useEffect, useState } from "react";
import {
  createWikiPage,
  deleteWikiPage,
  fetchWikiPage,
  updateWikiPage,
  wikiTree,
  type WikiTreeNode,
} from "@codeplan/api-client";

interface PageData {
  id: string;
  slug: string;
  title: string;
  content: string;
  html: string;
}

/** Wiki 页(T-133):文档树 + 页面查看/编辑。 */
export function WikiPageView({ projectId }: { projectId: string }) {
  const [tree, setTree] = useState<WikiTreeNode[] | null>(null);
  const [slug, setSlug] = useState<string | null>(null);
  const [page, setPage] = useState<PageData | null>(null);
  const [editing, setEditing] = useState(false);
  const [draftTitle, setDraftTitle] = useState("");
  const [draftContent, setDraftContent] = useState("");
  const [newSlug, setNewSlug] = useState("");
  const [error, setError] = useState<string | null>(null);

  const loadTree = useCallback(() => {
    wikiTree(projectId)
      .then(setTree)
      .catch((e: Error) => setError(e.message));
  }, [projectId]);

  useEffect(() => loadTree(), [loadTree]);

  const openPage = (targetSlug: string) => {
    setEditing(false);
    fetchWikiPage(projectId, targetSlug)
      .then((p) => {
        setPage(p);
        setSlug(targetSlug);
        setDraftTitle(p.title);
        setDraftContent(p.content);
      })
      .catch((e: Error) => setError(e.message));
  };

  const createPage = async () => {
    if (!newSlug.trim()) return;
    try {
      await createWikiPage(projectId, {
        slug: newSlug.trim(),
        title: newSlug.trim(),
        content: `# ${newSlug}\n`,
      });
      setNewSlug("");
      loadTree();
    } catch (e) {
      setError((e as Error).message);
    }
  };

  const save = async () => {
    if (!page) return;
    try {
      const updated = await updateWikiPage(projectId, page.slug, {
        title: draftTitle,
        content: draftContent,
      });
      setPage({ ...updated, html: renderClient(updated.content) });
      setEditing(false);
      loadTree();
    } catch (e) {
      setError((e as Error).message);
    }
  };

  const remove = async () => {
    if (!page) return;
    await deleteWikiPage(projectId, page.slug);
    setPage(null);
    setSlug(null);
    loadTree();
  };

  const renderTree = (nodes: WikiTreeNode[], depth = 0): React.ReactNode =>
    nodes.map((n) => (
      <div key={n.id}>
        <button
          className={`cp-nav-list-item${slug === n.slug ? " active" : ""}`}
          style={{ paddingLeft: 10 + depth * 14 }}
          onClick={() => openPage(n.slug)}
        >
          <span style={{ fontSize: 13 }}>📄</span>
          <span className="cp-nowrap">{n.title}</span>
        </button>
        {n.children.length > 0 && renderTree(n.children, depth + 1)}
      </div>
    ));

  return (
    <div className="cp-page">
      <div className="cp-page-head">
        <div>
          <h1 className="cp-page-title">Wiki</h1>
          <div className="cp-page-sub">项目文档与知识库</div>
        </div>
      </div>

      {error && <div className="cp-alert cp-alert-danger">{error}</div>}

      <div style={{ display: "flex", gap: "var(--cp-space-4)", alignItems: "flex-start" }}>
        <aside className="cp-sidebar">
          <div style={{ display: "flex", gap: 4, marginBottom: "var(--cp-space-2)" }}>
            <input
              placeholder="新页面 slug"
              className="cp-input-sm cp-input-mono"
              value={newSlug}
              onChange={(e) => setNewSlug(e.target.value)}
              style={{ flex: 1 }}
              onKeyDown={(e) => e.key === "Enter" && void createPage()}
            />
            <button className="cp-btn cp-btn-sm" onClick={() => void createPage()}>新建</button>
          </div>
          <div className="cp-nav-list">
            {tree ? (
              tree.length > 0 ? renderTree(tree) : (
                <div className="cp-muted" style={{ fontSize: 12, padding: 8 }}>暂无页面</div>
              )
            ) : (
              <div className="cp-muted" style={{ fontSize: 12, padding: 8 }}>加载中…</div>
            )}
          </div>
        </aside>

        <section className="cp-card" style={{ flex: 1, minWidth: 0 }}>
          {!page && (
            <div className="cp-empty">
              <div className="cp-empty-icon">📖</div>
              <div className="cp-empty-title">从左侧选择或新建页面</div>
              <div className="cp-empty-hint">Wiki 页面支持 Markdown 语法</div>
            </div>
          )}

          {page && !editing && (
            <>
              <div className="cp-card-head">
                <h3 style={{ flex: 1 }}>{page.title}</h3>
                <button className="cp-btn cp-btn-sm" onClick={() => setEditing(true)}>编辑</button>
                <button className="cp-btn cp-btn-sm danger" onClick={() => void remove()}>删除</button>
              </div>
              <div className="cp-card-body">
                <div className="wiki-html" dangerouslySetInnerHTML={{ __html: page.html }} />
              </div>
            </>
          )}

          {page && editing && (
            <div className="cp-card-body cp-col">
              <input
                value={draftTitle}
                onChange={(e) => setDraftTitle(e.target.value)}
                placeholder="标题"
                style={{ fontSize: 16, fontWeight: 600, height: 38 }}
              />
              <textarea
                value={draftContent}
                onChange={(e) => setDraftContent(e.target.value)}
                rows={18}
                className="cp-input-mono"
                style={{ width: "100%" }}
              />
              <div className="cp-row">
                <button className="cp-btn-primary" onClick={() => void save()}>保存</button>
                <button className="cp-btn" onClick={() => setEditing(false)}>取消</button>
              </div>
            </div>
          )}
        </section>
      </div>
    </div>
  );
}

/** 极简前端 Markdown 预览(保存后以服务端 HTML 为准)。 */
function renderClient(md: string): string {
  return md
    .split("\n\n")
    .map((block) =>
      block.startsWith("# ")
        ? `<h1>${block.slice(2)}</h1>`
        : block.startsWith("## ")
          ? `<h2>${block.slice(3)}</h2>`
          : `<p>${block.replace(/\n/g, "<br/>")}</p>`,
    )
    .join("\n");
}
