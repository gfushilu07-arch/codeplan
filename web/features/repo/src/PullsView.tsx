import { useCallback, useEffect, useState } from "react";
import {
  addPrComment,
  getPull,
  getPullDiff,
  listPrComments,
  listPulls,
  listReviewers,
  mergePull,
  postReview,
  prStatus,
  reviewSummary,
  transitionPull,
  createPull,
  type PrComment,
  type PullRequest,
  type ReviewSummary,
  type StatusSummary,
  type ReviewerEntry,
} from "@codeplan/api-client";

function StateBadge({ state }: { state: string }) {
  const cls =
    state === "open"
      ? "cp-badge cp-badge-open"
      : state === "merged"
        ? "cp-badge cp-badge-merged"
        : "cp-badge cp-badge-closed";
  const label = state === "open" ? "开启" : state === "merged" ? "已合并" : "已关闭";
  return <span className={cls}>{label}</span>;
}

/** PR 列表 + 详情(对话流/行内评论/diff/评审/合并)T-214。 */
export function PullsView({ owner, repoKey }: { owner: string; repoKey: string }) {
  const [pulls, setPulls] = useState<PullRequest[]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setPulls(await listPulls(owner, repoKey));
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    }
  }, [owner, repoKey]);

  useEffect(() => {
    void reload();
  }, [reload]);

  return (
    <div className="cp-col" style={{ gap: "var(--cp-space-4)" }}>
      <NewPullForm owner={owner} repoKey={repoKey} onCreated={() => void reload()} />
      {error && <div className="cp-alert cp-alert-danger">{error}</div>}

      <div className="cp-table-wrap">
        <table className="cp-table">
          <thead>
            <tr>
              <th style={{ width: 64 }}>编号</th>
              <th>标题</th>
              <th style={{ width: 240 }}>分支</th>
              <th style={{ width: 90 }}>状态</th>
              <th style={{ width: 90 }}></th>
            </tr>
          </thead>
          <tbody>
            {pulls.map((pr) => (
              <tr key={pr.number}>
                <td className="cp-mono cp-muted">#{pr.number}</td>
                <td>
                  {pr.draft && <span className="cp-badge cp-badge-draft" style={{ marginRight: 6 }}>草稿</span>}
                  <span style={{ fontWeight: 500 }}>{pr.title}</span>
                </td>
                <td className="cp-mono cp-muted" style={{ fontSize: 12 }}>
                  {pr.source_branch} → {pr.target_branch}
                </td>
                <td><StateBadge state={pr.state} /></td>
                <td>
                  <button className="cp-btn cp-btn-sm" onClick={() => setSelected(pr.number)}>查看</button>
                </td>
              </tr>
            ))}
            {pulls.length === 0 && (
              <tr>
                <td colSpan={5}>
                  <div className="cp-empty" style={{ padding: "var(--cp-space-8)" }}>
                    <div className="cp-empty-icon">⇄</div>
                    <div className="cp-empty-title">暂无 Pull Request</div>
                    <div className="cp-empty-hint">在上方填写源分支与标题即可发起</div>
                  </div>
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>

      {selected !== null && (
        <PrDetail
          owner={owner}
          repoKey={repoKey}
          number={selected}
          onClose={() => {
            setSelected(null);
            void reload();
          }}
        />
      )}
    </div>
  );
}

function NewPullForm({
  owner,
  repoKey,
  onCreated,
}: {
  owner: string;
  repoKey: string;
  onCreated: () => void;
}) {
  const [source, setSource] = useState("");
  const [title, setTitle] = useState("");
  const [draft, setDraft] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  const submit = async () => {
    if (!source.trim() || !title.trim()) return;
    try {
      await createPull(owner, repoKey, { title, source_branch: source, draft });
      setSource("");
      setTitle("");
      setDraft(false);
      setMsg(null);
      onCreated();
    } catch (e) {
      setMsg((e as Error).message);
    }
  };

  return (
    <div className="cp-card cp-card-pad">
      <div className="cp-toolbar">
        <input
          placeholder="源分支,如 feature/login"
          value={source}
          onChange={(e) => setSource(e.target.value)}
          style={{ width: 200 }}
        />
        <input
          placeholder="Pull Request 标题"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          style={{ flex: 1, minWidth: 220 }}
        />
        <label className="cp-check">
          <input type="checkbox" checked={draft} onChange={(e) => setDraft(e.target.checked)} /> 草稿
        </label>
        <button className="cp-btn-primary" onClick={() => void submit()}>新建 PR</button>
        {msg && <span style={{ color: "var(--cp-color-danger)", fontSize: 13 }}>{msg}</span>}
      </div>
    </div>
  );
}

function PrDetail({
  owner,
  repoKey,
  number,
  onClose,
}: {
  owner: string;
  repoKey: string;
  number: number;
  onClose: () => void;
}) {
  const [pr, setPr] = useState<PullRequest | null>(null);
  const [diff, setDiff] = useState("");
  const [comments, setComments] = useState<PrComment[]>([]);
  const [summary, setSummary] = useState<ReviewSummary | null>(null);
  const [status, setStatus] = useState<StatusSummary | null>(null);
  const [reviewers, setReviewers] = useState<ReviewerEntry[]>([]);
  const [commentText, setCommentText] = useState("");
  const [inlineTarget, setInlineTarget] = useState<{ file: string; line: number } | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  const reloadAll = useCallback(async () => {
    try {
      const [p, d, c, s, st, rv] = await Promise.all([
        getPull(owner, repoKey, number),
        getPullDiff(owner, repoKey, number).catch(() => ""),
        listPrComments(owner, repoKey, number),
        reviewSummary(owner, repoKey, number),
        prStatus(owner, repoKey, number),
        listReviewers(owner, repoKey, number),
      ]);
      setPr(p);
      setDiff(d);
      setComments(c);
      setSummary(s);
      setStatus(st);
      setReviewers(rv);
    } catch (e) {
      setMsg((e as Error).message);
    }
  }, [owner, repoKey, number]);

  useEffect(() => {
    void reloadAll();
  }, [reloadAll]);

  const act = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
      setMsg(null);
      await reloadAll();
    } catch (e) {
      setMsg((e as Error).message);
    }
  };

  const sendComment = async () => {
    if (!commentText.trim()) return;
    await act(async () => {
      await addPrComment(owner, repoKey, number, {
        body: commentText,
        ...(inlineTarget ? { file_path: inlineTarget.file, line: inlineTarget.line, side: "RIGHT" } : {}),
      });
      setCommentText("");
      setInlineTarget(null);
    });
  };

  if (!pr) return <div className="cp-card cp-card-pad cp-muted">加载中…</div>;

  return (
    <div className="cp-card">
      <div className="cp-card-head" style={{ gap: "var(--cp-space-3)" }}>
        <h3 style={{ flex: 1 }}>
          <span className="cp-muted" style={{ fontWeight: 400 }}>#{pr.number}</span> {pr.title}
        </h3>
        <StateBadge state={pr.state} />
        <button className="cp-btn cp-btn-sm" onClick={onClose}>关闭</button>
      </div>

      <div className="cp-card-body cp-col" style={{ gap: "var(--cp-space-3)" }}>
        <div className="cp-mono cp-muted" style={{ fontSize: 12 }}>
          {pr.source_branch} → {pr.target_branch}
        </div>

        <div className="cp-row" style={{ gap: "var(--cp-space-4)", flexWrap: "wrap", fontSize: 13 }}>
          {summary && (
            <span>
              评审{" "}
              <b
                style={{
                  color:
                    summary.combined === "approved"
                      ? "var(--cp-color-success-fg)"
                      : summary.combined === "changes_requested"
                        ? "var(--cp-color-danger-fg)"
                        : "var(--cp-color-text-muted)",
                }}
              >
                {summary.combined}
              </b>{" "}
              <span className="cp-muted">({summary.approvals.length} 通过 / {summary.changes_requested.length} 驳回)</span>
            </span>
          )}
          {status && status.combined !== "none" && (
            <span>
              检查{" "}
              <b style={{ color: status.combined === "success" ? "var(--cp-color-success-fg)" : "var(--cp-color-danger-fg)" }}>
                {status.combined}
              </b>{" "}
              <span className="cp-muted">
                ({status.checks.map((c) => `${c.context}:${c.state}`).join(", ")})
              </span>
            </span>
          )}
          {reviewers.length > 0 && (
            <span className="cp-muted">
              评审人:{reviewers.map((r) => `${r.reviewer.slice(0, 8)}${r.verdict ? `(${r.verdict})` : ""}`).join(", ")}
            </span>
          )}
        </div>

        {pr.state === "open" && (
          <div className="cp-toolbar">
            <button className="cp-btn success" onClick={() => void act(() => postReview(owner, repoKey, number, "approved", "通过"))}>
              ✓ 通过
            </button>
            <button className="cp-btn" onClick={() => void act(() => postReview(owner, repoKey, number, "changes_requested", "驳回"))}>
              ✗ 驳回
            </button>
            <span style={{ width: 1, height: 20, background: "var(--cp-color-border-light)" }} />
            <button className="cp-btn-primary" onClick={() => void act(() => mergePull(owner, repoKey, number, "merge"))}>合并 merge</button>
            <button className="cp-btn" onClick={() => void act(() => mergePull(owner, repoKey, number, "squash"))}>squash</button>
            <button className="cp-btn" onClick={() => void act(() => mergePull(owner, repoKey, number, "rebase"))}>rebase</button>
            <button className="cp-btn danger" onClick={() => void act(() => transitionPull(owner, repoKey, number, "close"))}>关闭 PR</button>
          </div>
        )}
        {pr.state === "closed" && (
          <div className="cp-toolbar">
            <button className="cp-btn-primary" onClick={() => void act(() => transitionPull(owner, repoKey, number, "reopen"))}>重开</button>
          </div>
        )}
        {msg && <div className="cp-alert cp-alert-danger">{msg}</div>}

        <div>
          <div className="cp-section-title">变更 Diff <span className="cp-muted" style={{ fontWeight: 400, fontSize: 12 }}>点击新增行可锚定行内评论</span></div>
          <div className="cp-diff">
            <div className="cp-diff-body">
              {diff.split("\n").map((line, i) => {
                const isHunk = line.startsWith("@@");
                const fileMatch = diff.slice(0, diff.indexOf(line)).match(/diff --git a\/(\S+) b\//);
                const clickable = line.startsWith("+") && !isHunk && !!fileMatch;
                const inlineHere =
                  inlineTarget && fileMatch && inlineTarget.file === fileMatch[1] && inlineTarget.line === i + 1;
                const kind = isHunk
                  ? "hunk"
                  : line.startsWith("+")
                    ? "add"
                    : line.startsWith("-")
                      ? "del"
                      : "";
                return (
                  <div
                    key={i}
                    className={`cp-diff-line ${kind}${clickable ? " pickable" : ""}${inlineHere ? " picked" : ""}`}
                    onClick={() => {
                      if (clickable && fileMatch) setInlineTarget({ file: fileMatch[1], line: i + 1 });
                    }}
                  >
                    {line || " "}
                    {inlineHere && <span style={{ color: "var(--cp-color-primary)" }}> ← 行内评论锚定</span>}
                  </div>
                );
              })}
              {!diff && <div className="cp-diff-line cp-muted">(无 diff)</div>}
            </div>
          </div>
        </div>

        <div>
          <div className="cp-section-title">对话流 <span className="cp-badge cp-badge-neutral">{comments.length}</span></div>
          <div className="cp-col" style={{ gap: "var(--cp-space-2)" }}>
            {comments.map((c) => (
              <div key={c.id} className="cp-card" style={{ boxShadow: "none" }}>
                <div className="cp-card-body" style={{ padding: "var(--cp-space-3)" }}>
                  <div className="cp-row" style={{ fontSize: 12 }}>
                    <span className="cp-avatar cp-avatar-sm">{c.author.slice(0, 1).toUpperCase()}</span>
                    <span className="cp-mono cp-muted">{c.author.slice(0, 8)}</span>
                    <span className="cp-muted">{new Date(c.created_at).toLocaleString()}</span>
                    {c.file_path && (
                      <span className="cp-tag cp-tag-mono" style={{ color: "var(--cp-color-purple)" }}>
                        📎 {c.file_path}:{c.line}
                      </span>
                    )}
                  </div>
                  <div style={{ whiteSpace: "pre-wrap", marginTop: 8 }}>{c.body}</div>
                </div>
              </div>
            ))}
            {comments.length === 0 && <div className="cp-muted" style={{ fontSize: 13 }}>暂无评论</div>}
          </div>
        </div>

        <div className="cp-row">
          <input
            placeholder={inlineTarget ? `行内评论:${inlineTarget.file}:${inlineTarget.line}` : "写下你的评论…"}
            value={commentText}
            onChange={(e) => setCommentText(e.target.value)}
            style={{ flex: 1 }}
            onKeyDown={(e) => {
              if (e.key === "Enter") void sendComment();
            }}
          />
          <button className="cp-btn-primary" onClick={() => void sendComment()}>发送</button>
          {inlineTarget && <button className="cp-btn" onClick={() => setInlineTarget(null)}>取消行内</button>}
        </div>
      </div>
    </div>
  );
}
