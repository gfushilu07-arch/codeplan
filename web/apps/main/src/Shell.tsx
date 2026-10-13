import type { MeInfo } from "@codeplan/api-client";

const NAV: Array<{ label: string; path: string }> = [
  { label: "工作台", path: "/" },
  { label: "项目", path: "/projects" },
  { label: "工作项", path: "/items" },
  { label: "需求池", path: "/backlog" },
  { label: "甘特", path: "/gantt" },
  { label: "代码", path: "/repos" },
  { label: "应用", path: "/apps" },
  { label: "Wiki", path: "/wiki" },
  { label: "报表", path: "/report" },
  { label: "管理", path: "/admin" },
];

function SearchIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <circle cx="11" cy="11" r="7" />
      <path d="m20 20-3.5-3.5" />
    </svg>
  );
}

/** 平台外壳:顶部导航(logo + 主导航 + 全局搜索 + 用户区)。 */
export function Shell({
  me,
  route,
  onLogout,
  children,
}: {
  me: MeInfo;
  route: string;
  onLogout: () => void;
  children?: React.ReactNode;
}) {
  const nav = me.is_platform_admin ? NAV : NAV.filter((item) => item.path !== "/admin");
  const avatarChar = (me.display_name || "?").slice(0, 1).toUpperCase();
  const openSearch = () =>
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true }));

  return (
    <div style={{ minHeight: "100vh", display: "flex", flexDirection: "column" }}>
      <header className="cp-header">
        <a className="cp-logo" href="#/">
          <span className="cp-logo-mark">CP</span>
          <span>CodePlan</span>
        </a>

        <nav className="cp-nav">
          {nav.map((item) => {
            const active =
              route === item.path || (item.path !== "/" && route.startsWith(item.path));
            return (
              <a
                key={item.path}
                href={`#${item.path}`}
                className={`cp-nav-item${active ? " active" : ""}`}
              >
                {item.label}
              </a>
            );
          })}
        </nav>

        <div className="cp-user">
          <button
            type="button"
            className="cp-search-trigger"
            onClick={openSearch}
            title="全局搜索"
          >
            <SearchIcon />
            <span>搜索工作项、Wiki…</span>
            <span className="cp-kbd cp-spacer">⌘K</span>
          </button>

          <button type="button" className="cp-user-chip" onClick={onLogout} title="点击登出">
            <span className="cp-avatar">{avatarChar}</span>
            <span className="cp-user-name">{me.display_name}</span>
          </button>
        </div>
      </header>

      <main className={`cp-container${route === "/" ? " cp-container-flush" : ""}`} style={{ width: "100%" }}>
        {children}
      </main>
    </div>
  );
}
