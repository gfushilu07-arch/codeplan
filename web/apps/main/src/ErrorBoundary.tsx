import { Component, type ReactNode } from "react";

interface Props {
  children: ReactNode;
}
interface State {
  error: Error | null;
}

/** 路由级错误边界:单页渲染异常时保留外壳,只降级当前内容区,避免整站白屏。 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error) {
    console.error("[CodePlan] 页面渲染失败:", error);
  }

  render() {
    if (this.state.error) {
      return (
        <div className="cp-card cp-card-pad">
          <div className="cp-alert cp-alert-danger" style={{ marginBottom: "var(--cp-space-3)" }}>
            页面渲染失败:{this.state.error.message}
          </div>
          <button className="cp-btn" onClick={() => this.setState({ error: null })}>
            重试
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
