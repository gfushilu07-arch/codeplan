import { useEffect, useState } from "react";
import { useHashRoute } from "./auth";
import { fetchMe, getToken, setToken, login, type MeInfo } from "@codeplan/api-client";
import { LoginPage } from "./LoginPage";
import { Shell } from "./Shell";
import { WorkbenchPage } from "./WorkbenchPage";
import { BoardViewPage } from "@codeplan/feature-workitem";
import { WikiPageView } from "@codeplan/feature-wiki";
import { RepoPage } from "@codeplan/feature-repo";

/** 应用根:未登录显示登录页,已登录进入外壳。 */
export function App() {
  const route = useHashRoute();
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
    <Shell me={me} route={route} onLogout={handleLogout}>
      {route === "/projects" && <BoardViewPage />}
      {route === "/wiki" && <WikiRoute />}
      {route === "/repos" && <RepoRoute />}
      {route !== "/projects" && route !== "/wiki" && route !== "/repos" && <WorkbenchPage />}
    </Shell>
  );
}


/** Wiki 路由包装:项目选择 + WikiPageView。 */
function WikiRoute() {
  const [projectId, setProjectId] = useState<string | null>(null);
  useEffect(() => {
    import("@codeplan/api-client").then(({ listProjects }) =>
      listProjects()
        .then((ps) => setProjectId(ps[0]?.id ?? null))
        .catch(() => setProjectId(null)),
    );
  }, []);
  if (!projectId) return <div style={{ color: "var(--cp-color-text-muted)" }}>暂无项目</div>;
  return <WikiPageView projectId={projectId} />;
}

/** 仓库路由包装:项目选择(取 CP 键演示,完整选择器随管理页)。 */
function RepoRoute() {
  const [projects, setProjects] = useState<Array<{ id: string; key: string }> | null>(null);
  useEffect(() => {
    import("@codeplan/api-client")
      .then(({ listProjects }) => listProjects().then(setProjects))
      .catch(() => setProjects([]));
  }, []);
  if (!projects || projects.length === 0) {
    return <div style={{ color: "var(--cp-color-text-muted)" }}>暂无项目</div>;
  }
  // v1:仓库固定挂在 admin 命名空间(owner 键暂以登录名占位,见 T-120 API)
  return (
    <RepoPage
      owner="admin"
      repoKey="demo"
      defaultBranch="main"
    />
  );
}