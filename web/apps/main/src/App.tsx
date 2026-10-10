import { useEffect, useState } from "react";
import { fetchMe, getToken, setToken, login, type MeInfo } from "@codeplan/api-client";
import { LoginPage } from "./LoginPage";
import { Shell } from "./Shell";
import { WorkbenchPage } from "./WorkbenchPage";

/** 应用根:未登录显示登录页,已登录进入外壳。 */
export function App() {
  const [me, setMe] = useState<MeInfo | null>(null);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    if (!getToken()) {
      setReady(true);
      return;
    }
    fetchMe()
      .then(setMe)
      .catch(() => setToken(null))
      .finally(() => setReady(true));
  }, []);

  const handleLogin = async (username: string, password: string) => {
    const session = await login(username, password);
    if (!session.token) throw new Error("登录响应缺少令牌");
    setToken(session.token);
    setMe({
      id: session.user_id,
      display_name: session.display_name,
      email: "",
      is_platform_admin: session.is_platform_admin,
    });
  };

  const handleLogout = () => {
    setToken(null);
    setMe(null);
  };

  if (!ready) {
    return <div style={{ padding: "var(--cp-space-4)", fontFamily: "var(--cp-font-sans)" }}>加载中…</div>;
  }
  if (!me) {
    return <LoginPage onLogin={handleLogin} />;
  }
  return (
    <Shell me={me} route="/" onLogout={handleLogout}>
      <WorkbenchPage />
    </Shell>
  );
}
