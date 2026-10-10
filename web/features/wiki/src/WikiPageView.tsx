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

/** Wiki 页(T-133):文档树 + 页面查看/编辑 + 附件入口(附件 API 见后端,T-115)。 */
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

  const renderTree = (nodes: WikiTreeNode[]): React.ReactNode =>
    nodes.map((n) => (
      <div key={n.id}>
        <div
          onClick={() => openPage(n.slug)}
          style={{
            cursor: "pointer",
            padding: "4px 8px",
            borderRadius: "var(--cp-radius-sm)",
            color: slug === n.slug ? "var(--cp-color-primary)" : "var(--cp-color-text)",
            fontSize: 13,
          }}
        >
          {n.title}
        </div>
        {n.children.length > 0 && (
          <div style={{ paddingLeft: "var(--cp-space-3)" }}>{renderTree(n.children)}</div>
        )}
      </div>
    ));

  return (
    <div style={{ display: "flex", gap: "var(--cp-space-3)", alignItems: "flex-start" }}>
      <aside
        style={{
          width: 220,
          background: "var(--cp-color-bg)",
          border: "1px solid var(--cp-color-border)",
          borderRadius: "var(--cp-radius-md)",
          padding: "var(--cp-space-2)",
        }}
      >
        <div style={{ display: "flex", gap: 4, marginBottom: "var(--cp-space-2)" }}>
          <input
            placeholder="新页面 slug"
            value={newSlug}
            onChange={(e) => setNewSlug(e.target.value)}
            style={{
              flex: 1,
              border: "1px solid var(--cp-color-border)",
              borderRadius: "var(--cp-radius-sm)",
              padding: "4px 6px",
              fontSize: 12,
              fontFamily: "var(--cp-font-mono)",
              background: "var(--cp-color-bg)",
              color: "var(--cp-color-text)",
            }}
          />
          <button onClick={createPage} style={{ font: "inherit", fontSize: 12, cursor: "pointer" }}>
            新建
          </button>
        </div>
        {tree ? renderTree(tree) : <div>加载中…</div>}
      </aside>
      <section style={{ flex: 1, background: "var(--cp-color-bg)", borderRadius: "var(--cp-radius-md)", padding: "var(--cp-space-3)" }}>
        {error && <div style={{ color: "var(--cp-color-danger)", fontSize: 13 }}>{error}</div>}
        {!page && <div style={{ color: "var(--cp-color-text-muted)" }}>从左侧选择或新建页面</div>}
        {page && !editing && (
          <>
            <div style={{ display: "flex", alignItems: "center", gap: "var(--cp-space-2)" }}>
              <h2 style={{ fontSize: 18, margin: 0 }}>{page.title}</h2>
              <button onClick={() => setEditing(true)} style={{ font: "inherit", fontSize: 13, cursor: "pointer" }}>
                编辑
              </button>
              <button onClick={remove} style={{ font: "inherit", fontSize: 13, cursor: "pointer", color: "var(--cp-color-danger)" }}>
                删除
              </button>
            </div>
            <div className="wiki-html" dangerouslySetInnerHTML={{ __html: page.html }} />
          </>
        )}
        {page && editing && (
          <div style={{ display: "flex", flexDirection: "column", gap: "var(--cp-space-2)" }}>
            <input
              value={draftTitle}
              onChange={(e) => setDraftTitle(e.target.value)}
              placeholder="标题"
              style={{ font: "inherit", padding: "var(--cp-space-2)" }}
            />
            <textarea
              value={draftContent}
              onChange={(e) => setDraftContent(e.target.value)}
              rows={16}
              style={{
                fontFamily: "var(--cp-font-mono)",
                fontSize: 13,
                padding: "var(--cp-space-2)",
                width: "100%",
                boxSizing: "border-box",
              }}
            />
            <div style={{ display: "flex", gap: "var(--cp-space-2)" }}>
              <button onClick={save} style={{ font: "inherit", cursor: "pointer" }}>
                保存
              </button>
              <button onClick={() => setEditing(false)} style={{ font: "inherit", cursor: "pointer" }}>
                取消
              </button>
            </div>
          </div>
        )}
      </section>
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
