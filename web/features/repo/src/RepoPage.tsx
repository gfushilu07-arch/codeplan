import { useEffect, useState } from "react";
import {
  repoBlob,
  repoCommits,
  repoTree,
  type RepoCommit,
  type RepoTreeEntry,
} from "@codeplan/api-client";

type Tab = "files" | "commits";

/** 仓库页(T-134):文件树/提交历史/blob 查看;克隆地址(带令牌)复制入口。 */
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
  const [error, setError] = useState<string | null>(null);

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

  const openBlob = async (path: string) => {
    try {
      const content = await repoBlob(owner, repoKey, reference, path);
      setSelected({ path, content });
    } catch (e) {
      setError((e as Error).message);
    }
  };

  const cloneUrl = `http://localhost:8081/git/${owner}/${repoKey}.git`;

  return (
    <div>
      <div style={{ display: "flex", alignItems: "center", gap: "var(--cp-space-2)", marginBottom: "var(--cp-space-3)" }}>
        <h2 style={{ fontSize: 18, margin: 0 }}>
          {owner}/{repoKey}
        </h2>
        {(["files", "commits"] as Tab[]).map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            style={{
              font: "inherit",
              fontSize: 13,
              cursor: "pointer",
              border: "1px solid var(--cp-color-border)",
              borderRadius: "var(--cp-radius-md)",
              padding: "4px 10px",
              background: tab === t ? "var(--cp-color-primary)" : "transparent",
              color: tab === t ? "var(--cp-color-primary-fg)" : "var(--cp-color-text-muted)",
            }}
          >
            {t === "files" ? "文件" : "提交"}
          </button>
        ))}
        <span style={{ marginLeft: "auto", fontFamily: "var(--cp-font-mono)", fontSize: 12, color: "var(--cp-color-text-muted)" }}>
          git clone {cloneUrl}
        </span>
      </div>
      {error && <div style={{ color: "var(--cp-color-danger)", fontSize: 13 }}>{error}</div>}

      {tab === "files" && (
        <div style={{ display: "flex", gap: "var(--cp-space-3)", alignItems: "flex-start" }}>
          <ul style={{ listStyle: "none", padding: 0, flex: 1 }}>
            {entries.map((e) => (
              <li
                key={e.oid + e.name}
                onClick={() => e.kind === "blob" && openBlob(e.name)}
                style={{
                  display: "flex",
                  gap: "var(--cp-space-2)",
                  alignItems: "center",
                  padding: "6px 10px",
                  borderBottom: "1px solid var(--cp-color-border)",
                  cursor: e.kind === "blob" ? "pointer" : "default",
                  fontSize: 13,
                }}
              >
                <span style={{ fontFamily: "var(--cp-font-mono)", fontSize: 11, color: "var(--cp-color-text-muted)" }}>
                  {e.kind}
                </span>
                <span style={{ color: "var(--cp-color-text)" }}>{e.name}</span>
                {e.size != null && (
                  <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--cp-color-text-muted)" }}>
                    {e.size} B
                  </span>
                )}
              </li>
            ))}
            {entries.length === 0 && <li style={{ color: "var(--cp-color-text-muted)" }}>空目录</li>}
          </ul>
          {selected && (
            <pre
              style={{
                flex: 1,
                background: "var(--cp-color-bg)",
                border: "1px solid var(--cp-color-border)",
                borderRadius: "var(--cp-radius-md)",
                padding: "var(--cp-space-2)",
                fontFamily: "var(--cp-font-mono)",
                fontSize: 12,
                overflow: "auto",
                maxHeight: 480,
              }}
            >
              {selected.content}
            </pre>
          )}
        </div>
      )}

      {tab === "commits" && (
        <ul style={{ listStyle: "none", padding: 0, display: "flex", flexDirection: "column", gap: "var(--cp-space-2)" }}>
          {commits.map((c) => (
            <li
              key={c.id}
              style={{
                background: "var(--cp-color-bg)",
                border: "1px solid var(--cp-color-border)",
                borderRadius: "var(--cp-radius-md)",
                padding: "var(--cp-space-2) var(--cp-space-3)",
              }}
            >
              <div>{c.summary}</div>
              <div style={{ fontSize: 12, color: "var(--cp-color-text-muted)", fontFamily: "var(--cp-font-mono)" }}>
                {c.id.slice(0, 8)} · {c.author_name} · {new Date(c.time * 1000).toLocaleDateString()}
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
