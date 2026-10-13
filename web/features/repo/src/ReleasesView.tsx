import { useCallback, useEffect, useState } from "react";
import {
  createTag,
  listBranchProtection,
  listReleases,
  listTags,
  publishRelease,
  setBranchProtection,
  type ReleaseInfo,
  type TagBrief,
} from "@codeplan/api-client";

/** Tags / Releases / 分支保护(T-208 / T-205 / T-214)。 */
export function ReleasesView({ owner, repoKey }: { owner: string; repoKey: string }) {
  const [tags, setTags] = useState<TagBrief[]>([]);
  const [releases, setReleases] = useState<ReleaseInfo[]>([]);
  const [protection, setProtection] = useState<unknown[]>([]);
  const [tagName, setTagName] = useState("");
  const [tagMsg, setTagMsg] = useState<string | null>(null);
  const [protPattern, setProtPattern] = useState("main");
  const [minApprovals, setMinApprovals] = useState(1);
  const [requireOwner, setRequireOwner] = useState(false);
  const [protMsg, setProtMsg] = useState<string | null>(null);

  const reload = useCallback(async () => {
    const [t, r, p] = await Promise.all([
      listTags(owner, repoKey),
      listReleases(owner, repoKey),
      listBranchProtection(owner, repoKey).catch(() => [] as unknown[]),
    ]);
    setTags(t);
    setReleases(r);
    setProtection(p);
  }, [owner, repoKey]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const addTag = async () => {
    if (!tagName.trim()) return;
    try {
      await createTag(owner, repoKey, { name: tagName.trim(), message: `tag ${tagName}` });
      setTagName("");
      setTagMsg(null);
      await reload();
    } catch (e) {
      setTagMsg((e as Error).message);
    }
  };

  const publish = async (tag: string) => {
    try {
      await publishRelease(owner, repoKey, { tag_name: tag, name: tag });
      await reload();
    } catch (e) {
      setTagMsg((e as Error).message);
    }
  };

  const saveProtection = async () => {
    try {
      await setBranchProtection(owner, repoKey, {
        branch_pattern: protPattern,
        min_approvals: minApprovals,
        required_contexts: [],
        block_force_push: true,
        block_direct_push: false,
        require_code_owner: requireOwner,
      });
      setProtMsg(null);
      await reload();
    } catch (e) {
      setProtMsg((e as Error).message);
    }
  };

  return (
    <div className="cp-col" style={{ gap: "var(--cp-space-4)" }}>
      {/* Tags */}
      <div className="cp-card">
        <div className="cp-card-head">
          <h3>标签</h3>
          <span className="cp-badge cp-badge-neutral">{tags.length}</span>
        </div>
        <div className="cp-card-body cp-col">
          <div className="cp-toolbar">
            <input
              placeholder="v1.0.0"
              className="cp-input-mono"
              value={tagName}
              onChange={(e) => setTagName(e.target.value)}
              style={{ width: 150 }}
            />
            <button className="cp-btn-primary" onClick={() => void addTag()}>打标签(默认分支)</button>
            {tagMsg && <span style={{ color: "var(--cp-color-danger)", fontSize: 13 }}>{tagMsg}</span>}
          </div>
          {tags.length === 0 ? (
            <div className="cp-muted" style={{ fontSize: 13 }}>暂无标签</div>
          ) : (
            <div className="cp-table-wrap">
              <table className="cp-table">
                <thead>
                  <tr>
                    <th>标签</th>
                    <th style={{ width: 140 }}>提交</th>
                    <th style={{ width: 140 }}></th>
                  </tr>
                </thead>
                <tbody>
                  {tags.map((t) => (
                    <tr key={t.name}>
                      <td className="cp-mono">{t.name}</td>
                      <td className="cp-mono cp-muted">{t.commit_sha.slice(0, 8)}</td>
                      <td>
                        {releases.every((r) => r.tag !== t.name) && (
                          <button className="cp-btn cp-btn-sm" onClick={() => void publish(t.name)}>发布 Release</button>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      </div>

      {/* Releases */}
      <div className="cp-card">
        <div className="cp-card-head">
          <h3>Releases</h3>
          <span className="cp-badge cp-badge-neutral">{releases.length}</span>
        </div>
        <div className="cp-card-body cp-col">
          {releases.length === 0 && <div className="cp-muted" style={{ fontSize: 13 }}>暂无 Release</div>}
          {releases.map((r) => (
            <div key={r.tag} className="cp-card" style={{ boxShadow: "none" }}>
              <div className="cp-card-body" style={{ padding: "var(--cp-space-3)" }}>
                <div className="cp-row">
                  <b>{r.name}</b>
                  <span className="cp-tag cp-tag-mono">{r.tag}</span>
                  {r.notes_auto && <span className="cp-badge cp-badge-info">自动 Notes</span>}
                </div>
                <pre style={{ whiteSpace: "pre-wrap", fontSize: 13, margin: "10px 0 0", fontFamily: "var(--cp-font-sans)" }}>{r.body}</pre>
                {r.assets.length > 0 && (
                  <div className="cp-muted" style={{ fontSize: 12, marginTop: 8 }}>
                    附件:{r.assets.map((a) => `${a.filename}(${a.size}B)`).join(", ")}
                  </div>
                )}
              </div>
            </div>
          ))}
        </div>
      </div>

      {/* 分支保护 */}
      <div className="cp-card">
        <div className="cp-card-head">
          <h3>分支保护</h3>
        </div>
        <div className="cp-card-body cp-col">
          <div className="cp-toolbar">
            <input
              value={protPattern}
              onChange={(e) => setProtPattern(e.target.value)}
              style={{ width: 140 }}
              placeholder="分支模式"
              className="cp-input-mono"
            />
            <label className="cp-check">
              最少通过
              <input
                type="number"
                min={0}
                max={10}
                value={minApprovals}
                onChange={(e) => setMinApprovals(Number(e.target.value))}
                style={{ width: 56 }}
              />
            </label>
            <label className="cp-check">
              <input type="checkbox" checked={requireOwner} onChange={(e) => setRequireOwner(e.target.checked)} /> 所有者评审
            </label>
            <button className="cp-btn-primary" onClick={() => void saveProtection()}>保存规则</button>
            {protMsg && <span style={{ color: "var(--cp-color-danger)", fontSize: 13 }}>{protMsg}</span>}
          </div>
          {protection.length > 0 && (
            <div className="cp-col" style={{ gap: 6 }}>
              {protection.map((p) => {
                const rule = p as { branch_pattern: string; min_approvals: number; block_force_push: boolean; block_direct_push: boolean; require_code_owner: boolean };
                return (
                  <div key={rule.branch_pattern} className="cp-row" style={{ fontSize: 13 }}>
                    <span className="cp-tag cp-tag-mono">{rule.branch_pattern}</span>
                    <span className="cp-muted">
                      最少 {rule.min_approvals} 通过
                      {rule.require_code_owner && " · 所有者评审"}
                      {rule.block_force_push && " · 禁 force push"}
                      {rule.block_direct_push && " · 禁直推"}
                    </span>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
