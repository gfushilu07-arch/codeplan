import { useEffect, useState } from "react";
import { useHashRoute } from "./auth";
import {
  fetchMe,
  getToken,
  setToken,
  login,
  type MeInfo,
} from "@codeplan/api-client";
import { LoginPage } from "./LoginPage";
import { Shell } from "./Shell";
import { ErrorBoundary } from "./ErrorBoundary";
import { WorkbenchPage } from "./WorkbenchPage";
import { BoardViewPage } from "@codeplan/feature-workitem";
import { WikiPageView } from "@codeplan/feature-wiki";
import { RepoPage } from "@codeplan/feature-repo";
import { AdminPage } from "@codeplan/feature-admin";
import { GanttView, BacklogView, WorkItemDetail, WorkItemListView } from "@codeplan/feature-workitem";
import { ReportsView } from "@codeplan/feature-insights";
import { CommandPalette } from "@codeplan/feature-search";
import { AppCenterPage } from "@codeplan/feature-appcenter";

/** 应用根:未登录显示登录页,已登录进入外壳(会话经 /api/me 恢复)。 */
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
    return (
      <div className="cp-container cp-muted" style={{ paddingTop: 80, textAlign: "center" }}>
        加载中…
      </div>
    );
  }
  if (!me) {
    return <LoginPage onLogin={handleLogin} />;
  }
  return (
    <Shell me={me} route={route} onLogout={handleLogout}>
      <CommandPalette onNavigate={(hash) => (window.location.hash = hash)} />
      <ErrorBoundary key={route}>
        {route === "/projects" && <BoardViewPage />}
        {route === "/wiki" && <WikiRoute />}
        {route === "/repos" && <RepoRoute />}
        {route === "/admin" && me.is_platform_admin && <AdminPage />}
        {route === "/admin" && !me.is_platform_admin && (
          <div className="cp-alert cp-alert-danger">仅平台管理员可访问</div>
        )}
        {route === "/gantt" && <GanttView />}
        {route === "/backlog" && <BacklogView />}
        {route === "/items" && <WorkItemListView />}
        {route === "/apps" && <AppCenterPage />}
        {route.startsWith("/item/") && <WorkItemDetail itemId={route.slice("/item/".length)} />}
        {route === "/report" && <ReportsView />}
        {route === "/" && <WorkbenchPage />}
      </ErrorBoundary>
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
  if (!projectId)
    return (
      <div className="cp-empty">
        <div className="cp-empty-icon">📦</div>
        <div className="cp-empty-title">暂无项目</div>
        <div className="cp-empty-hint">请先创建一个项目</div>
      </div>
    );
  return <WikiPageView projectId={projectId} />;
}

/** 仓库路由包装(v1:仓库挂 admin 命名空间,选择器随 T-135 管理页完善)。 */
function RepoRoute() {
  const [projects, setProjects] = useState<Array<{ id: string; key: string }> | null>(null);
  useEffect(() => {
    import("@codeplan/api-client")
      .then(({ listProjects }) => listProjects().then(setProjects))
      .catch(() => setProjects([]));
  }, []);
  if (!projects || projects.length === 0) {
    return (
      <div className="cp-empty">
        <div className="cp-empty-icon">📦</div>
        <div className="cp-empty-title">暂无项目</div>
        <div className="cp-empty-hint">请先创建一个项目</div>
      </div>
    );
  }
  return <RepoPage owner="admin" repoKey="pr-demo" defaultBranch="main" />;
}
