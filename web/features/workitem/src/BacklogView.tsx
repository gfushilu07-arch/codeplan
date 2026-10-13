import { useEffect, useState } from "react";
import {
  fetchItemsTree,
  listProjects,
  type TreeItem,
} from "@codeplan/api-client";
import { statusLabel, statusBadgeClass } from "./status";

const TYPE_META: Record<string, { icon: string; label: string }> = {
  epic: { icon: "🎯", label: "史诗" },
  story: { icon: "📖", label: "需求" },
  task: { icon: "✅", label: "任务" },
  bug: { icon: "🐛", label: "缺陷" },
};

function TreeNode({ node, depth }: { node: TreeItem; depth: number }) {
  const meta = TYPE_META[node.item_type] ?? { icon: "•", label: node.item_type };
  return (
    <div>
      <div
        className="cp-tree-row"
        style={{ marginLeft: depth * 22, cursor: "pointer" }}
        onClick={() => (window.location.hash = `/item/${node.id}`)}
      >
        <span style={{ fontSize: 14 }}>{meta.icon}</span>
        <code className="cp-muted" style={{ fontSize: 12 }}>{node.display_key}</code>
        <span className="cp-nowrap" style={{ flex: 1 }}>{node.title}</span>
        <span className="cp-tag">{meta.label}</span>
        <span className={statusBadgeClass(node.status)}>{statusLabel(node.status)}</span>
      </div>
      {node.children.map((child) => (
        <TreeNode key={child.id} node={child} depth={depth + 1} />
      ))}
    </div>
  );
}

/** 需求池(T-215):epic → story → task 父子树。 */
export function BacklogView() {
  const [tree, setTree] = useState<TreeItem[] | null>(null);
  const [projectKey, setProjectKey] = useState("RPT");
  const [projects, setProjects] = useState<Array<{ key: string }>>([]);

  useEffect(() => {
    listProjects()
      .then((ps) => setProjects(ps.map((p) => ({ key: (p as unknown as { key: string }).key }))))
      .catch(() => setProjects([]));
  }, []);

  useEffect(() => {
    setTree(null);
    fetchItemsTree(projectKey)
      .then(setTree)
      .catch(() => setTree([]));
  }, [projectKey]);

  const count = tree ? tree.reduce((acc, n) => acc + 1 + n.children.length, 0) : 0;

  return (
    <div className="cp-page">
      <div className="cp-page-head">
        <div>
          <h1 className="cp-page-title">需求池</h1>
          <div className="cp-page-sub">史诗 → 需求 → 任务 的父子层级视图</div>
        </div>
        <div className="cp-page-actions">
          <select value={projectKey} onChange={(e) => setProjectKey(e.target.value)}>
            {projects.map((p) => (
              <option key={p.key} value={p.key}>{p.key}</option>
            ))}
          </select>
          <span className="cp-badge cp-badge-neutral">共 {count} 项</span>
        </div>
      </div>

      <div className="cp-card">
        {tree === null ? (
          <div className="cp-card-body cp-col">
            <div className="cp-skeleton" style={{ height: 36 }} />
            <div className="cp-skeleton" style={{ height: 36 }} />
            <div className="cp-skeleton" style={{ height: 36 }} />
          </div>
        ) : tree.length === 0 ? (
          <div className="cp-empty">
            <div className="cp-empty-icon">☰</div>
            <div className="cp-empty-title">暂无工作项</div>
            <div className="cp-empty-hint">切换到其他项目或先创建需求</div>
          </div>
        ) : (
          <div style={{ padding: "var(--cp-space-2)" }}>
            {tree.map((node) => (
              <TreeNode key={node.id} node={node} depth={0} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
