import { useState } from "react";

const inputStyle: React.CSSProperties = {
  border: "1px solid var(--cp-color-border)",
  borderRadius: "var(--cp-radius-md)",
  padding: "var(--cp-space-2)",
  font: "inherit",
  background: "var(--cp-color-bg)",
  color: "var(--cp-color-text)",
  width: "100%",
  boxSizing: "border-box",
};

const submitStyle: React.CSSProperties = {
  border: "none",
  borderRadius: "var(--cp-radius-md)",
  padding: "var(--cp-space-2) var(--cp-space-3)",
  background: "var(--cp-color-primary)",
  color: "var(--cp-color-primary-fg)",
  font: "inherit",
  cursor: "pointer",
};

/** 登录页(会话经 JWT;TOTP/OIDC 入口随批次二接入)。 */
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
    <div
      style={{
        minHeight: "100vh",
        background: "var(--cp-color-surface)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        fontFamily: "var(--cp-font-sans)",
      }}
    >
      <form
        onSubmit={submit}
        style={{
          background: "var(--cp-color-bg)",
          borderRadius: "var(--cp-radius-lg)",
          boxShadow: "var(--cp-shadow-2)",
          padding: "var(--cp-space-4)",
          width: 360,
          display: "flex",
          flexDirection: "column",
          gap: "var(--cp-space-3)",
        }}
      >
        <h1 style={{ margin: 0, fontSize: 22, color: "var(--cp-color-text)" }}>CodePlan 登录</h1>
        <label style={labelStyle}>
          用户名
          <input
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            style={inputStyle}
            autoComplete="username"
          />
        </label>
        <label style={labelStyle}>
          密码
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            style={inputStyle}
            autoComplete="current-password"
          />
        </label>
        {error && (
          <div style={{ color: "var(--cp-color-danger)", fontSize: 13 }} role="alert">
            {error}
          </div>
        )}
        <button type="submit" disabled={busy} style={submitStyle}>
          {busy ? "登录中…" : "登录"}
        </button>
      </form>
    </div>
  );
}

const labelStyle: React.CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 4,
  fontSize: 13,
  color: "var(--cp-color-text)",
};
