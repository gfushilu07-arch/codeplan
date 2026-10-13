import { lazy, Suspense, useEffect, useMemo, useState } from "react";
import {
  fetchProjectEfficiency,
  fetchReport,
  fetchSprintReport,
  listProjects,
  listSprints,
  type ProjectEfficiency,
} from "@codeplan/api-client";
import { token } from "./chartTokens";

// ECharts 体积较大(约 290KB gzip):懒加载,仅在打开报表页时载入,避免拖慢首屏
const Chart = lazy(() => import("./Chart").then((m) => ({ default: m.Chart })));

interface TrendPoint {
  date: string;
  created: number;
  closed: number;
}
interface ThroughputPoint {
  week: string;
  done: number;
}
interface CfdPoint {
  date: string;
  created: number;
  done: number;
  wip: number;
}
interface LoadRow {
  assignee: string;
  open_items: number;
  open_points: number;
}
interface SprintReport {
  total: number;
  done: number;
  open: number;
  points_total: number;
  points_done: number;
  done_ratio: number;
}

/** 懒加载图表(加载中占位,避免布局跳动)。 */
function LazyChart({ option, height = 240 }: { option: Parameters<typeof Chart>[0]["option"]; height?: number }) {
  return (
    <Suspense fallback={<div className="cp-skeleton" style={{ height }} />}>
      <Chart option={option} height={height} />
    </Suspense>
  );
}

/** 秒 → 人类可读(小时/天)。 */
function humanSeconds(sec: number | null): string {
  if (sec == null) return "—";
  if (sec < 60) return `${sec} 秒`;
  if (sec < 3600) return `${(sec / 60).toFixed(1)} 分钟`;
  if (sec < 86400) return `${(sec / 3600).toFixed(1)} 小时`;
  return `${(sec / 86400).toFixed(1)} 天`;
}

/** 报表页(T-215/T-308):ECharts 图表 + 效能指标(交付周期/缺陷逃逸)。 */
export function ReportsView() {
  const [projectKey, setProjectKey] = useState("RPT");
  const [projects, setProjects] = useState<Array<{ key: string }>>([]);
  const [bugs, setBugs] = useState<TrendPoint[] | null>(null);
  const [throughput, setThroughput] = useState<ThroughputPoint[]>([]);
  const [cfd, setCfd] = useState<CfdPoint[]>([]);
  const [load, setLoad] = useState<LoadRow[]>([]);
  const [sprints, setSprints] = useState<Array<{ id: string; name: string }>>([]);
  const [sprintId, setSprintId] = useState<string>("");
  const [sprint, setSprint] = useState<SprintReport | null>(null);
  const [eff, setEff] = useState<ProjectEfficiency | null>(null);

  useEffect(() => {
    listProjects()
      .then((ps) => setProjects(ps.map((p) => ({ key: (p as unknown as { key: string }).key }))))
      .catch(() => setProjects([]));
  }, []);

  useEffect(() => {
    setBugs(null);
    fetchReport(projectKey, "bugs", 14).then((d) => setBugs(d as TrendPoint[])).catch(() => setBugs([]));
    fetchReport(projectKey, "throughput", 6).then((d) => setThroughput(d as ThroughputPoint[])).catch(() => setThroughput([]));
    fetchReport(projectKey, "cfd", 14).then((d) => setCfd(d as CfdPoint[])).catch(() => setCfd([]));
    fetchReport(projectKey, "load").then((d) => setLoad(d as LoadRow[])).catch(() => setLoad([]));
    fetchProjectEfficiency(projectKey).then(setEff).catch(() => setEff(null));
    listSprints(projectKey).then(setSprints).catch(() => setSprints([]));
  }, [projectKey]);

  useEffect(() => {
    if (sprints.length === 0) return;
    setSprintId((cur) => cur || sprints[0].id);
  }, [sprints]);

  useEffect(() => {
    if (!sprintId) return;
    fetchSprintReport(projectKey, sprintId).then(setSprint).catch(() => setSprint(null));
  }, [projectKey, sprintId]);

  const grid = token("--cp-color-border-light", "#f1f1f8");
  const axis = token("--cp-color-text-muted", "#9c9db4");
  const primary = token("--cp-color-primary", "#165dff");
  const danger = token("--cp-color-danger", "#da203e");
  const success = token("--cp-color-success", "#088f4e");

  const bugOption = useMemo(
    () => ({
      grid: { left: 40, right: 16, top: 28, bottom: 28 },
      tooltip: { trigger: "axis" as const },
      legend: { data: ["新建", "完结"], top: 0, textStyle: { color: axis, fontSize: 12 } },
      xAxis: {
        type: "category" as const,
        data: [...(bugs ?? [])].reverse().map((p) => p.date.slice(5)),
        axisLine: { lineStyle: { color: grid } },
        axisLabel: { color: axis, fontSize: 11 },
      },
      yAxis: {
        type: "value" as const,
        splitLine: { lineStyle: { color: grid } },
        axisLabel: { color: axis, fontSize: 11 },
      },
      series: [
        {
          name: "新建",
          type: "line" as const,
          smooth: true,
          areaStyle: { opacity: 0.12 },
          itemStyle: { color: danger },
          data: [...(bugs ?? [])].reverse().map((p) => p.created),
        },
        {
          name: "完结",
          type: "line" as const,
          smooth: true,
          areaStyle: { opacity: 0.12 },
          itemStyle: { color: success },
          data: [...(bugs ?? [])].reverse().map((p) => p.closed),
        },
      ],
    }),
    [bugs, grid, axis, danger, success],
  );

  const throughputOption = useMemo(
    () => ({
      grid: { left: 40, right: 16, top: 20, bottom: 28 },
      tooltip: { trigger: "axis" as const },
      xAxis: {
        type: "category" as const,
        data: throughput.map((p) => p.week),
        axisLine: { lineStyle: { color: grid } },
        axisLabel: { color: axis, fontSize: 11 },
      },
      yAxis: {
        type: "value" as const,
        splitLine: { lineStyle: { color: grid } },
        axisLabel: { color: axis, fontSize: 11 },
      },
      series: [
        {
          name: "完结",
          type: "bar" as const,
          barMaxWidth: 28,
          itemStyle: { color: primary, borderRadius: [4, 4, 0, 0] },
          data: throughput.map((p) => p.done),
        },
      ],
    }),
    [throughput, grid, axis, primary],
  );

  const cfdOption = useMemo(
    () => ({
      grid: { left: 40, right: 16, top: 28, bottom: 28 },
      tooltip: { trigger: "axis" as const },
      legend: { data: ["累计新建", "累计完结", "在制"], top: 0, textStyle: { color: axis, fontSize: 12 } },
      xAxis: {
        type: "category" as const,
        data: cfd.map((p) => p.date.slice(5)),
        axisLine: { lineStyle: { color: grid } },
        axisLabel: { color: axis, fontSize: 11 },
      },
      yAxis: {
        type: "value" as const,
        splitLine: { lineStyle: { color: grid } },
        axisLabel: { color: axis, fontSize: 11 },
      },
      series: [
        {
          name: "累计新建",
          type: "line" as const,
          stack: "flow",
          areaStyle: { opacity: 0.25 },
          itemStyle: { color: primary },
          data: cfd.map((p) => p.created),
        },
        {
          name: "累计完结",
          type: "line" as const,
          stack: "flow",
          areaStyle: { opacity: 0.25 },
          itemStyle: { color: success },
          data: cfd.map((p) => p.done),
        },
        {
          name: "在制",
          type: "line" as const,
          smooth: true,
          itemStyle: { color: danger },
          data: cfd.map((p) => p.wip),
        },
      ],
    }),
    [cfd, grid, axis, primary, success, danger],
  );

  const loadOption = useMemo(
    () => ({
      grid: { left: 90, right: 30, top: 16, bottom: 24 },
      tooltip: { trigger: "axis" as const },
      xAxis: {
        type: "value" as const,
        splitLine: { lineStyle: { color: grid } },
        axisLabel: { color: axis, fontSize: 11 },
      },
      yAxis: {
        type: "category" as const,
        data: load.map((r) => r.assignee.slice(0, 8)),
        axisLine: { lineStyle: { color: grid } },
        axisLabel: { color: axis, fontSize: 11 },
      },
      series: [
        {
          name: "未完成项",
          type: "bar" as const,
          barMaxWidth: 16,
          itemStyle: { color: primary, borderRadius: [0, 4, 4, 0] },
          data: load.map((r) => r.open_items),
        },
      ],
    }),
    [load, grid, axis, primary],
  );

  return (
    <div className="cp-page">
      <div className="cp-page-head">
        <div>
          <h1 className="cp-page-title">效能报表</h1>
          <div className="cp-page-sub">迭代、缺陷、吞吐、交付周期与成员负载</div>
        </div>
        <div className="cp-page-actions">
          <select value={projectKey} onChange={(e) => setProjectKey(e.target.value)}>
            {projects.map((p) => (
              <option key={p.key} value={p.key}>{p.key}</option>
            ))}
          </select>
        </div>
      </div>

      {/* 迭代报告 */}
      <div className="cp-card">
        <div className="cp-card-head">
          <h3>迭代报告</h3>
          <div className="cp-spacer" />
          <select value={sprintId} onChange={(e) => setSprintId(e.target.value)}>
            {sprints.map((s) => (
              <option key={s.id} value={s.id}>{s.name}</option>
            ))}
          </select>
        </div>
        <div className="cp-card-body">
          {sprint ? (
            <>
              <div className="cp-stats" style={{ marginBottom: "var(--cp-space-4)" }}>
                <div className="cp-stat" style={{ boxShadow: "none" }}>
                  <div className="cp-stat-label">总计</div>
                  <div className="cp-stat-value">{sprint.total}</div>
                </div>
                <div className="cp-stat" style={{ boxShadow: "none" }}>
                  <div className="cp-stat-label">完成</div>
                  <div className="cp-stat-value" style={{ color: "var(--cp-color-success)" }}>{sprint.done}</div>
                </div>
                <div className="cp-stat" style={{ boxShadow: "none" }}>
                  <div className="cp-stat-label">未完成</div>
                  <div className="cp-stat-value">{sprint.open}</div>
                </div>
                <div className="cp-stat" style={{ boxShadow: "none" }}>
                  <div className="cp-stat-label">故事点</div>
                  <div className="cp-stat-value">
                    {sprint.points_done}
                    <span className="cp-muted" style={{ fontSize: 15, fontWeight: 400 }}>/{sprint.points_total}</span>
                  </div>
                </div>
              </div>
              <div className="cp-row" style={{ gap: "var(--cp-space-3)" }}>
                <span style={{ fontSize: 13, minWidth: 64 }}>完成率</span>
                <span className="cp-progress" style={{ flex: 1 }}>
                  <span style={{ width: `${Math.min(100, sprint.done_ratio)}%` }} />
                </span>
                <b style={{ width: 56, textAlign: "right" }}>{sprint.done_ratio}%</b>
              </div>
            </>
          ) : (
            <div className="cp-muted" style={{ fontSize: 13 }}>无迭代数据</div>
          )}
        </div>
      </div>

      {/* 交付与质量(T-308) */}
      <div className="cp-card">
        <div className="cp-card-head">
          <h3>交付与质量</h3>
          <span className="cp-muted" style={{ fontSize: 12 }}>交付周期 · 缺陷逃逸</span>
        </div>
        <div className="cp-card-body">
          <div className="cp-stats">
            <div className="cp-stat" style={{ boxShadow: "none" }}>
              <div className="cp-stat-label">交付周期(中位)</div>
              <div className="cp-stat-value">{humanSeconds(eff?.delivery_cycle.median_seconds ?? null)}</div>
              <div className="cp-stat-foot">样本 {eff?.delivery_cycle.count ?? 0} 项(终态工作项)</div>
            </div>
            <div className="cp-stat" style={{ boxShadow: "none" }}>
              <div className="cp-stat-label">交付周期(p85)</div>
              <div className="cp-stat-value">{humanSeconds(eff?.delivery_cycle.p85_seconds ?? null)}</div>
              <div className="cp-stat-foot">长尾参考</div>
            </div>
            <div className="cp-stat" style={{ boxShadow: "none" }}>
              <div className="cp-stat-label">缺陷逃逸率</div>
              <div
                className="cp-stat-value"
                style={{ color: (eff?.defect_escape.rate_percent ?? 0) > 0 ? "var(--cp-color-danger)" : "var(--cp-color-success)" }}
              >
                {eff ? `${eff.defect_escape.rate_percent}%` : "—"}
              </div>
              <div className="cp-stat-foot">
                迭代结束后才暴露 {eff?.defect_escape.escaped ?? 0}/{eff?.defect_escape.total ?? 0}
              </div>
            </div>
          </div>
        </div>
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(420px, 1fr))", gap: "var(--cp-space-4)" }}>
        <div className="cp-card">
          <div className="cp-card-head">
            <h3>缺陷趋势</h3>
            <span className="cp-muted" style={{ fontSize: 12 }}>近 14 天</span>
          </div>
          <div className="cp-card-body">
            {bugs === null ? (
              <div className="cp-muted" style={{ fontSize: 13 }}>加载中…</div>
            ) : bugs.length === 0 ? (
              <div className="cp-muted" style={{ fontSize: 13 }}>暂无数据</div>
            ) : (
              <LazyChart option={bugOption} height={220} />
            )}
          </div>
        </div>

        <div className="cp-card">
          <div className="cp-card-head">
            <h3>吞吐</h3>
            <span className="cp-muted" style={{ fontSize: 12 }}>每周完结</span>
          </div>
          <div className="cp-card-body">
            {throughput.length === 0 ? (
              <div className="cp-muted" style={{ fontSize: 13 }}>暂无数据</div>
            ) : (
              <LazyChart option={throughputOption} height={220} />
            )}
          </div>
        </div>

        <div className="cp-card">
          <div className="cp-card-head">
            <h3>累计流</h3>
            <span className="cp-muted" style={{ fontSize: 12 }}>近 14 天</span>
          </div>
          <div className="cp-card-body">
            {cfd.length === 0 ? (
              <div className="cp-muted" style={{ fontSize: 13 }}>暂无数据</div>
            ) : (
              <LazyChart option={cfdOption} height={220} />
            )}
          </div>
        </div>

        <div className="cp-card">
          <div className="cp-card-head">
            <h3>成员负载</h3>
            <span className="cp-badge cp-badge-neutral">{load.length}</span>
          </div>
          <div className="cp-card-body">
            {load.length === 0 ? (
              <div className="cp-muted" style={{ fontSize: 13 }}>暂无未完成项</div>
            ) : (
              <LazyChart option={loadOption} height={Math.max(160, load.length * 32)} />
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
