import { useState } from "react";

/** 登录页:左侧品牌区 + 右侧表单卡(会话经 JWT;TOTP/OIDC 入口随批次二接入)。 */
export function LoginPage({ onLogin }: { onLogin: (u: string, p: string) => Promise<void> }) {
  const [username, setUsername] = useState("admin");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await onLogin(username, password);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div style={{ minHeight: "100vh", display: "flex" }}>
      {/* 品牌区 */}
      <div
        className="cp-login-brand"
        style={{
          flex: "1 1 46%",
          display: "flex",
          flexDirection: "column",
          justifyContent: "space-between",
          padding: "var(--cp-space-10)",
          color: "#fff",
          background:
            "radial-gradient(900px 500px at 15% 0%, #4d8bff 0%, transparent 55%), linear-gradient(150deg, #165dff 0%, #2951e0 55%, #344899 100%)",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, fontSize: 18, fontWeight: 700 }}>
          <span
            className="cp-logo-mark"
            style={{ background: "rgba(255,255,255,0.18)", boxShadow: "none", width: 30, height: 30 }}
          >
            CP
          </span>
          CodePlan
        </div>
        <div style={{ maxWidth: 420 }}>
          <h1 style={{ fontSize: 32, color: "#fff", lineHeight: 1.3, marginBottom: 14 }}>
            一站式研发效能平台
          </h1>
          <p style={{ fontSize: 15, opacity: 0.9, lineHeight: 1.8 }}>
            项目管理 · 代码托管 · 持续集成 · 制品与部署 —— 让团队从需求到上线的全链路在一个平台内闭环。
          </p>
          <div style={{ display: "flex", gap: 24, marginTop: 28, fontSize: 13, opacity: 0.9 }}>
            <span>✓ 需求 / 迭代 / 甘特</span>
            <span>✓ Git + PR 评审</span>
            <span>✓ 灰度发布</span>
          </div>
        </div>
        <div style={{ fontSize: 12, opacity: 0.7 }}>© 2026 CodePlan · 企业内网私有部署</div>
      </div>

      {/* 表单区 */}
      <div style={{ flex: "1 1 54%", display: "flex", alignItems: "center", justifyContent: "center", background: "var(--cp-color-bg)" }}>
        <form
          onSubmit={submit}
          style={{ width: 360, display: "flex", flexDirection: "column", gap: "var(--cp-space-4)" }}
        >
          <div>
            <h2 style={{ fontSize: 22 }}>欢迎回来</h2>
            <p className="cp-muted" style={{ fontSize: 13, marginTop: 6 }}>
              使用企业账号登录 CodePlan
            </p>
          </div>

          <div className="cp-field">
            <label htmlFor="login-user">用户名</label>
            <input
              id="login-user"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              autoComplete="username"
              style={{ height: 38 }}
            />
          </div>

          <div className="cp-field">
            <label htmlFor="login-pass">密码</label>
            <input
              id="login-pass"
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              autoComplete="current-password"
              placeholder="请输入密码"
              style={{ height: 38 }}
            />
          </div>

          {error && <div className="cp-alert cp-alert-danger" role="alert">{error}</div>}

          <button type="submit" className="cp-btn-primary cp-btn-lg cp-btn-block" disabled={busy}>
            {busy ? "登录中…" : "登 录"}
          </button>
        </form>
      </div>
    </div>
  );
}
