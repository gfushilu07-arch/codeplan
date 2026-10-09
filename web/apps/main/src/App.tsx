import { Button } from "@codeplan/ui";

/** 骨架占位首页:T-131(main shell)替换为真实布局。 */
export function App() {
  return (
    <div
      style={{
        minHeight: "100vh",
        background: "var(--cp-color-bg)",
        color: "var(--cp-color-text)",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: "var(--cp-space-3)",
        fontFamily: "var(--cp-font-sans)",
      }}
    >
      <h1 style={{ fontSize: 28, margin: 0 }}>CodePlan</h1>
      <p style={{ color: "var(--cp-color-text-muted)", margin: 0 }}>
        内部 PaaS + 研发管理闭环 · 骨架已就绪
      </p>
      <div style={{ display: "flex", gap: "var(--cp-space-2)" }}>
        <Button>主按钮</Button>
        <Button variant="secondary">次按钮</Button>
      </div>
    </div>
  );
}
