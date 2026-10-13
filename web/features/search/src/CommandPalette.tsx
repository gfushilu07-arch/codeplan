import { useEffect, useRef, useState } from "react";
import { fetchSearch, type SearchHit } from "@codeplan/api-client";

const NAV_COMMANDS: Array<{ label: string; target: string; icon: string }> = [
  { label: "工作台", target: "#/", icon: "⌂" },
  { label: "项目看板", target: "#/projects", icon: "▦" },
  { label: "工作项", target: "#/items", icon: "☰" },
  { label: "需求池", target: "#/backlog", icon: "⑃" },
  { label: "甘特图", target: "#/gantt", icon: "▤" },
  { label: "代码仓库", target: "#/repos", icon: "⎇" },
  { label: "Wiki", target: "#/wiki", icon: "📖" },
  { label: "效能报表", target: "#/report", icon: "📊" },
  { label: "应用中心", target: "#/apps", icon: "🚀" },
];

/** Cmd+K 命令面板(T-215):全文搜索(tantivy)+ 快速导航。 */
export function CommandPalette({ onNavigate }: { onNavigate: (hash: string) => void }) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [searching, setSearching] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setOpen((v) => !v);
      }
      if (e.key === "Escape") setOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    if (open) setTimeout(() => inputRef.current?.focus(), 50);
    else {
      setQuery("");
      setHits([]);
    }
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const q = query.trim();
    if (q.length < 2) {
      setHits([]);
      return;
    }
    setSearching(true);
    const timer = setTimeout(() => {
      fetchSearch(q, undefined, 8)
        .then(setHits)
        .catch(() => setHits([]))
        .finally(() => setSearching(false));
    }, 250);
    return () => clearTimeout(timer);
  }, [query, open]);

  if (!open) return null;

  const go = (hash: string) => {
    setOpen(false);
    onNavigate(hash);
  };

  const navMatches = NAV_COMMANDS.filter((c) =>
    query.trim() ? c.label.toLowerCase().includes(query.trim().toLowerCase()) : true,
  );

  return (
    <div className="cp-modal-backdrop" onClick={() => setOpen(false)}>
      <div className="cp-modal" onClick={(e) => e.stopPropagation()}>
        <div className="cp-modal-search">
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="var(--cp-color-text-muted)" strokeWidth="2" strokeLinecap="round" aria-hidden>
            <circle cx="11" cy="11" r="7" />
            <path d="m20 20-3.5-3.5" />
          </svg>
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="搜索工作项 / Wiki,或跳转页面…"
          />
          <span className="cp-kbd">Esc</span>
        </div>

        <div className="cp-modal-body">
          {navMatches.length > 0 && (
            <>
              <div className="cp-cmd-group">快速跳转</div>
              {navMatches.map((c) => (
                <div key={c.target} className="cp-cmd-item" onClick={() => go(c.target)}>
                  <span style={{ width: 18, textAlign: "center", color: "var(--cp-color-text-muted)" }}>{c.icon}</span>
                  <span>{c.label}</span>
                  <span className="cp-spacer cp-kbd">↵</span>
                </div>
              ))}
            </>
          )}

          {query.trim().length >= 2 && (
            <>
              <div className="cp-cmd-group">
                {searching ? "搜索中…" : `全文搜索 · ${hits.length} 条结果`}
              </div>
              {hits.map((h) => (
                <div key={h.id} className="cp-cmd-item" style={{ cursor: "default" }}>
                  <span style={{ width: 18, textAlign: "center" }}>
                    {h.kind === "wiki" ? "📄" : h.kind === "code" ? "⌨" : "✅"}
                  </span>
                  <div style={{ minWidth: 0 }}>
                    <div className="cp-nowrap">{h.title}</div>
                    {h.snippet && (
                      <div className="cp-muted" style={{ fontSize: 12 }}>{h.snippet}</div>
                    )}
                  </div>
                  <span className="cp-spacer cp-mono cp-muted">{h.score.toFixed(2)}</span>
                </div>
              ))}
              {!searching && hits.length === 0 && (
                <div className="cp-cmd-item cp-muted" style={{ cursor: "default" }}>无结果</div>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
