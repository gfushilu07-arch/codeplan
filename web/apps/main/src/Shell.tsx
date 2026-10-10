import type { MeInfo } from "@codeplan/api-client";

const NAV: Array<{ label: string; path: string }> = [
  { label: "工作台", path: "/" },
  { label: "项目", path: "/projects" },
  { label: "代码", path: "/repos" },
  { label: "管理", path: "/admin" },
];

const ghostButton: React.CSSProperties = {
  border: "1px solid var(--cp-color-border)",
  borderRadius: "var(--cp-radius-md)",
  background: "transparent",
  color: "var(--cp-color-text-muted)",
  font: "inherit",
  fontSize: 13,
  cursor: "pointer",
};

/** 平台外壳:顶部主导航 + 内容区(布局学 Gitee,视觉学 GitCode,见 ARCHITECTURE §2.3)。 */
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
  return (
    <div
      style={{
        minHeight: "100vh",
        background: "var(--cp-color-surface)",
        fontFamily: "var(--cp-font-sans)",
      }}
    >
      <nav
        style={{
          display: "flex",
          alignItems: "center",
          gap: "var(--cp-space-3)",
          padding: "var(--cp-space-2) var(--cp-space-4)",
          background: "var(--cp-color-bg)",
          borderBottom: "1px solid var(--cp-color-border)",
          position: "sticky",
          top: 0,
        }}
      >
        <strong style={{ color: "var(--cp-color-primary)", fontSize: 16 }}>CodePlan</strong>
        {NAV.map((item) => {
          const active = route === item.path || (item.path !== "/" && route.startsWith(item.path));
          return (
            <a
              key={item.path}
              href={`#${item.path}`}
              style={{
                color: active ? "var(--cp-color-primary)" : "var(--cp-color-text-muted)",
                textDecoration: "none",
                fontSize: 14,
                fontWeight: active ? 600 : 400,
              }}
            >
              {item.label}
            </a>
          );
        })}
        <span style={{ marginLeft: "auto", fontSize: 13, color: "var(--cp-color-text-muted)" }}>
          {me.display_name}
        </span>
        <button onClick={onLogout} style={ghostButton}>
          登出
        </button>
      </nav>
      <main style={{ padding: "var(--cp-space-4)", color: "var(--cp-color-text)", maxWidth: 1280, margin: "0 auto" }}>
        {children}
      </main>
    </div>
  );
}
