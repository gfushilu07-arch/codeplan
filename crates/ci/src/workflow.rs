//! Workflow YAML 解析(T-301):GitHub Actions 兼容子集。
//!
//! 支持范围(v1 明确收敛):
//! - 顶层:`name`、`on`(push.branches / manual)、`jobs`;
//! - job:`name`、`runs-on`(标签)、`steps`、`strategy.matrix`;
//! - step:`name`、`uses`(白名单)、`run`、`with`(字面量)、`env`(字面量)。
//!
//! 超范围字段**明示报错**(不静默忽略):container/services/needs/
//! timeout-minutes/if/strategy.fail-fast/environment/outputs/secrets 等。

use serde_json::Value as Json;

/// 解析/校验错误(带字段路径,面向用户)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowError {
    /// 错误位置(如 `jobs.build.steps[2]`)。
    pub path: String,
    /// 说明。
    pub message: String,
}

impl WorkflowError {
    fn new(path: &str, message: impl Into<String>) -> Self {
        Self {
            path: path.to_owned(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for WorkflowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

impl std::error::Error for WorkflowError {}

/// 触发配置。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Triggers {
    /// push 触发的分支过滤(空 = 全部分支)。
    pub push_branches: Vec<String>,
    /// 是否允许手动触发。
    pub manual: bool,
}

/// 一个 step。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Step {
    /// 名称(缺省取 uses 短名或 run 首行)。
    pub name: String,
    /// Action 引用(白名单校验;与 run 互斥)。
    pub uses: Option<String>,
    /// Shell 命令(与 uses 互斥)。
    pub run: Option<String>,
    /// with 输入(仅 uses 步骤;值须为字面量)。
    pub with: Vec<(String, String)>,
    /// 步骤级 env(字面量)。
    pub env: Vec<(String, String)>,
}

/// 矩阵维度(键 → 值列表)。
pub type Matrix = Vec<(String, Vec<String>)>;

/// 一个 job。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Job {
    /// job id(YAML 键)。
    pub id: String,
    /// 展示名。
    pub name: String,
    /// runner 标签(如 `ubuntu-latest`;调度侧按标签选 runner,T-302)。
    pub runs_on: String,
    /// 步骤(矩阵变量占位 `${{ matrix.x }}` 在展开时替换)。
    pub steps: Vec<Step>,
    /// 矩阵(空 = 单次执行)。
    pub matrix: Matrix,
}

/// 解析后的 workflow。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Workflow {
    /// 名称(缺省 = 文件所在仓库路径展示)。
    pub name: String,
    /// 触发。
    pub triggers: Triggers,
    /// jobs(保持 YAML 声明顺序)。
    pub jobs: Vec<Job>,
}

/// 支持的 uses 白名单(官方 action 子集;其余明示拒绝)。
pub const SUPPORTED_ACTIONS: [&str; 4] = [
    "actions/checkout",
    "actions/setup-node",
    "actions/setup-python",
    "actions/cache",
];

/// 已知超范围字段 → 提示信息。
const UNSUPPORTED_TOP: &[(&str, &str)] = &[
    ("env", "顶层 env 暂不支持(请下放到 step.env)"),
    ("concurrency", "concurrency 暂不支持"),
    ("permissions", "permissions 暂不支持"),
    ("defaults", "defaults 暂不支持"),
    ("schedule", "定时触发暂不支持(T-317 排期)"),
    ("workflow_dispatch", "请改用 on.manual"),
    ("workflow_call", "可复用 workflow 暂不支持"),
];

const UNSUPPORTED_JOB: &[(&str, &str)] = &[
    ("needs", "job 依赖(needs)暂不支持 — v1 各 job 独立执行"),
    ("container", "job 容器暂不支持(T-302 runner 容器化)"),
    ("services", "services 暂不支持"),
    ("environment", "environment 暂不支持"),
    ("outputs", "job outputs 暂不支持"),
    ("if", "条件执行暂不支持"),
    ("timeout-minutes", "超时配置暂不支持(默认 30 分钟)"),
    ("strategy.fail-fast", "fail-fast 暂不支持(固定 fail-fast)"),
    ("strategy.max-parallel", "max-parallel 暂不支持(固定串行)"),
];

const UNSUPPORTED_STEP: &[(&str, &str)] = &[
    ("if", "条件执行暂不支持"),
    ("working-directory", "working-directory 暂不支持"),
    ("shell", "shell 指定暂不支持(固定 sh)"),
    ("continue-on-error", "continue-on-error 暂不支持"),
    ("timeout-minutes", "超时配置暂不支持"),
    ("id", "step id 暂不支持(无表达式引用)"),
];

fn obj_keys(map: &serde_yaml::Mapping) -> Vec<String> {
    map.iter()
        .filter_map(|(k, _)| k.as_str().map(str::to_owned))
        .collect()
}

fn unsupported(path: &str, keys: &[String], table: &[(&str, &str)]) -> Result<(), WorkflowError> {
    for key in keys {
        for (bad, hint) in table {
            if key == bad {
                return Err(WorkflowError::new(
                    path,
                    format!("暂不支持 `{key}` — {hint}"),
                ));
            }
        }
    }
    Ok(())
}

/// 解析并校验 workflow YAML。
///
/// # Errors
/// YAML 语法错误;结构非法;超范围字段(明示)。
pub fn parse_workflow(yaml: &str) -> Result<Workflow, WorkflowError> {
    let root: serde_yaml::Value = serde_yaml::from_str(yaml)
        .map_err(|e| WorkflowError::new("(root)", format!("YAML 语法错误:{e}")))?;
    let Some(map) = root.as_mapping() else {
        return Err(WorkflowError::new("(root)", "顶层必须是映射"));
    };
    let top_keys = obj_keys(map);

    // 超范围明示报错
    unsupported("(root)", &top_keys, UNSUPPORTED_TOP)?;
    for key in &top_keys {
        if key.starts_with("on:") || key == "push" {
            return Err(WorkflowError::new(
                "(root)",
                format!(
                    "未识别的顶层字段 `{key}`(on 触发请用映射语法:on:\\n  push:\\n    branches: […])"
                ),
            ));
        }
    }

    // name
    let name = map
        .get(serde_yaml::Value::String("name".into()))
        .and_then(|v| v.as_str())
        .unwrap_or("workflow")
        .to_owned();

    // on
    let triggers = parse_triggers(map.get(serde_yaml::Value::String("on".into())))?;

    // jobs
    let Some(jobs_value) = map.get(serde_yaml::Value::String("jobs".into())) else {
        return Err(WorkflowError::new("(root)", "缺少 jobs"));
    };
    let jobs_map = jobs_value
        .as_mapping()
        .ok_or_else(|| WorkflowError::new("jobs", "jobs 必须是映射(job_id → 配置)"))?;
    if jobs_map.is_empty() {
        return Err(WorkflowError::new("jobs", "至少需要一个 job"));
    }

    let mut jobs = Vec::new();
    for (job_key, job_value) in jobs_map {
        let id = job_key
            .as_str()
            .ok_or_else(|| WorkflowError::new("jobs", "job id 必须是字符串"))?
            .to_owned();
        let job = parse_job(&id, job_value)?;
        jobs.push(job);
    }

    Ok(Workflow {
        name,
        triggers,
        jobs,
    })
}

fn parse_triggers(value: Option<&serde_yaml::Value>) -> Result<Triggers, WorkflowError> {
    let Some(value) = value else {
        return Err(WorkflowError::new("on", "缺少 on(触发配置)"));
    };
    let Some(map) = value.as_mapping() else {
        // `on: push` 简写
        if value.as_str() == Some("push") {
            return Ok(Triggers {
                push_branches: vec![],
                manual: false,
            });
        }
        return Err(WorkflowError::new("on", "on 必须是映射或 push"));
    };
    let mut triggers = Triggers {
        push_branches: vec![],
        manual: false,
    };
    for (k, v) in map {
        let key = k
            .as_str()
            .ok_or_else(|| WorkflowError::new("on", "触发键必须是字符串"))?;
        match key {
            "push" => {
                if let Some(m) = v.as_mapping() {
                    let branches = m.get(serde_yaml::Value::String("branches".into()));
                    if let Some(list) = branches {
                        let Some(seq) = list.as_sequence() else {
                            return Err(WorkflowError::new("on.push.branches", "必须是字符串数组"));
                        };
                        for b in seq {
                            let Some(s) = b.as_str() else {
                                return Err(WorkflowError::new(
                                    "on.push.branches",
                                    "分支必须是字符串",
                                ));
                            };
                            triggers.push_branches.push(s.to_owned());
                        }
                    }
                } else if v.is_null() || v.as_str() == Some("push") {
                    // on: { push: } 或 push: null → 全分支
                } else {
                    return Err(WorkflowError::new("on.push", "仅支持 branches 过滤"));
                }
            }
            "manual" => {
                triggers.manual = v.as_bool().unwrap_or(true);
            }
            other => {
                return Err(WorkflowError::new(
                    "on",
                    format!("暂不支持触发器 `{other}`(支持:push.branches / manual)"),
                ));
            }
        }
    }
    Ok(triggers)
}

fn parse_job(id: &str, value: &serde_yaml::Value) -> Result<Job, WorkflowError> {
    let path = format!("jobs.{id}");
    let Some(map) = value.as_mapping() else {
        return Err(WorkflowError::new(&path, "job 必须是映射"));
    };
    let keys = obj_keys(map);
    unsupported(&path, &keys, UNSUPPORTED_JOB)?;
    // strategy.fail-fast 等嵌套键在 strategy 下单独检查
    if let Some(strategy) = map.get(serde_yaml::Value::String("strategy".into()))
        && let Some(sm) = strategy.as_mapping()
    {
        let skeys = obj_keys(sm);
        unsupported(
            &format!("{path}.strategy"),
            &skeys,
            &[
                ("fail-fast", "fail-fast 暂不支持(固定 fail-fast)"),
                ("max-parallel", "max-parallel 暂不支持(固定串行)"),
            ],
        )?;
    }

    let name = map
        .get(serde_yaml::Value::String("name".into()))
        .and_then(|v| v.as_str())
        .unwrap_or(id)
        .to_owned();
    let runs_on = map
        .get(serde_yaml::Value::String("runs-on".into()))
        .and_then(|v| v.as_str())
        .ok_or_else(|| WorkflowError::new(&path, "缺少 runs-on"))?
        .to_owned();

    // matrix
    let mut matrix: Matrix = Vec::new();
    if let Some(strategy) = map
        .get(serde_yaml::Value::String("strategy".into()))
        .and_then(|s| s.as_mapping())
        .and_then(|s| s.get(serde_yaml::Value::String("matrix".into())))
    {
        let Some(mm) = strategy.as_mapping() else {
            return Err(WorkflowError::new(
                &format!("{path}.strategy.matrix"),
                "matrix 必须是映射",
            ));
        };
        for (k, v) in mm {
            let dim = k.as_str().ok_or_else(|| {
                WorkflowError::new(&format!("{path}.strategy.matrix"), "维度键必须是字符串")
            })?;
            let Some(seq) = v.as_sequence() else {
                return Err(WorkflowError::new(
                    &format!("{path}.strategy.matrix.{dim}"),
                    "维度值必须是数组",
                ));
            };
            let mut values = Vec::new();
            for item in seq {
                let s = scalar_to_string(item).ok_or_else(|| {
                    WorkflowError::new(
                        &format!("{path}.strategy.matrix.{dim}"),
                        "矩阵值必须是标量(字符串/数字/布尔)",
                    )
                })?;
                values.push(s);
            }
            if values.is_empty() {
                return Err(WorkflowError::new(
                    &format!("{path}.strategy.matrix.{dim}"),
                    "矩阵维度不能为空",
                ));
            }
            matrix.push((dim.to_owned(), values));
        }
    }

    // steps
    let Some(steps_value) = map.get(serde_yaml::Value::String("steps".into())) else {
        return Err(WorkflowError::new(&path, "缺少 steps"));
    };
    let Some(seq) = steps_value.as_sequence() else {
        return Err(WorkflowError::new(
            &format!("{path}.steps"),
            "steps 必须是数组",
        ));
    };
    let mut steps = Vec::new();
    for (i, sv) in seq.iter().enumerate() {
        let step = parse_step(&format!("{path}.steps[{i}]"), sv)?;
        steps.push(step);
    }
    if steps.is_empty() {
        return Err(WorkflowError::new(
            &format!("{path}.steps"),
            "至少需要一个 step",
        ));
    }

    Ok(Job {
        id: id.to_owned(),
        name,
        runs_on,
        steps,
        matrix,
    })
}

fn parse_step(path: &str, value: &serde_yaml::Value) -> Result<Step, WorkflowError> {
    let Some(map) = value.as_mapping() else {
        return Err(WorkflowError::new(path, "step 必须是映射"));
    };
    let keys = obj_keys(map);
    unsupported(path, &keys, UNSUPPORTED_STEP)?;

    let uses = map
        .get(serde_yaml::Value::String("uses".into()))
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let run = map
        .get(serde_yaml::Value::String("run".into()))
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    match (&uses, &run) {
        (Some(_), Some(_)) => {
            return Err(WorkflowError::new(path, "uses 与 run 只能取其一"));
        }
        (None, None) => {
            return Err(WorkflowError::new(path, "step 需要 uses 或 run"));
        }
        _ => {}
    }
    if let Some(u) = &uses {
        let base = u.split('@').next().unwrap_or(u);
        if !SUPPORTED_ACTIONS.contains(&base) {
            return Err(WorkflowError::new(
                path,
                format!(
                    "action `{u}` 不在支持列表(可用:{})",
                    SUPPORTED_ACTIONS.join(", ")
                ),
            ));
        }
    }

    let name = map
        .get(serde_yaml::Value::String("name".into()))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            uses.clone().unwrap_or_else(|| {
                run.as_ref()
                    .and_then(|r| r.lines().next())
                    .unwrap_or("step")
                    .to_owned()
            })
        });

    let with = parse_kv(path, map, "with")?;
    let env = parse_kv(path, map, "env")?;

    Ok(Step {
        name,
        uses,
        run,
        with,
        env,
    })
}

fn parse_kv(
    path: &str,
    map: &serde_yaml::Mapping,
    key: &str,
) -> Result<Vec<(String, String)>, WorkflowError> {
    let mut out = Vec::new();
    if let Some(v) = map.get(serde_yaml::Value::String(key.into())) {
        let Some(m) = v.as_mapping() else {
            return Err(WorkflowError::new(&format!("{path}.{key}"), "必须是映射"));
        };
        for (k, val) in m {
            let k = k
                .as_str()
                .ok_or_else(|| WorkflowError::new(&format!("{path}.{key}"), "键必须是字符串"))?;
            let s = scalar_to_string(val).ok_or_else(|| {
                WorkflowError::new(
                    &format!("{path}.{key}.{k}"),
                    "值必须是标量(表达式/嵌套结构暂不支持)",
                )
            })?;
            out.push((k.to_owned(), s));
        }
    }
    Ok(out)
}

fn scalar_to_string(v: &serde_yaml::Value) -> Option<String> {
    match v {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// 矩阵展开:`${{ matrix.x }}` 占位替换;返回各组合的 (替换值表, job 标识后缀)。
#[must_use]
pub fn expand_matrix(job: &Job) -> Vec<(Vec<(String, String)>, String)> {
    if job.matrix.is_empty() {
        return vec![(vec![], String::new())];
    }
    let mut combos: Vec<(Vec<(String, String)>, String)> = vec![(vec![], String::new())];
    for (dim, values) in &job.matrix {
        let mut next = Vec::new();
        for (base, suffix) in &combos {
            for v in values {
                let mut m = base.clone();
                m.push((dim.clone(), v.clone()));
                let s = format!("{suffix}-{dim}-{v}");
                next.push((m, s));
            }
        }
        combos = next;
    }
    combos
}

/// 矩阵变量替换(仅 `${{ matrix.dim }}` 形式)。
#[must_use]
pub fn render_matrix_placeholders(text: &str, values: &[(String, String)]) -> String {
    let mut out = text.to_owned();
    for (dim, v) in values {
        out = out.replace(&format!("${{{{ matrix.{dim} }}}}"), v);
    }
    out
}

/// workflow → JSON 摘要(入库存储形态)。
#[must_use]
pub fn to_json(wf: &Workflow) -> Json {
    serde_json::to_value(wf).unwrap_or(Json::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
name: CI
on:
  push:
    branches: [main, develop]
  manual: true
jobs:
  build:
    name: Build & Test
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: 测试
        run: |
          cargo test --workspace
      - run: cargo build --release
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 2
      - run: cargo clippy
"#;

    #[test]
    fn parses_valid_workflow() {
        let wf = parse_workflow(VALID).unwrap();
        assert_eq!(wf.name, "CI");
        assert_eq!(wf.triggers.push_branches, vec!["main", "develop"]);
        assert!(wf.triggers.manual);
        assert_eq!(wf.jobs.len(), 2);
        assert_eq!(wf.jobs[0].id, "build");
        assert_eq!(wf.jobs[0].steps.len(), 3);
        assert_eq!(wf.jobs[0].steps[1].name, "测试");
        assert!(
            wf.jobs[0].steps[2]
                .run
                .as_deref()
                .unwrap()
                .contains("cargo build")
        );
        assert_eq!(
            wf.jobs[1].steps[0].with,
            vec![("fetch-depth".to_owned(), "2".to_owned())]
        );
    }

    #[test]
    fn rejects_unsupported_fields_explicitly() {
        // 顶层超范围
        let err = parse_workflow("on:\n  push:\n  jobs: {}\nenv:\n  A: 1\n").unwrap_err();
        assert!(err.message.contains("env 暂不支持"), "{err}");

        // job 级 needs
        let err = parse_workflow("on:\n  push:\njobs:\n  b:\n    needs: a\n    runs-on: x\n    steps:\n      - run: echo\n")
            .unwrap_err();
        assert!(err.message.contains("needs"), "{err}");

        // step 级 if
        let err = parse_workflow(
            "on:\n  push:\njobs:\n  a:\n    runs-on: x\n    steps:\n      - run: echo\n        if: true\n",
        )
        .unwrap_err();
        assert!(err.message.contains("if"), "{err}");
    }

    #[test]
    fn rejects_unknown_action_and_missing_fields() {
        let err = parse_workflow(
            "on:\n  push:\njobs:\n  a:\n    runs-on: x\n    steps:\n      - uses: someone/unknown@v1\n",
        )
        .unwrap_err();
        assert!(err.message.contains("不在支持列表"), "{err}");

        let err = parse_workflow("on:\n  push:\njobs: {}\n").unwrap_err();
        assert!(err.message.contains("至少需要"), "{err}");

        let err = parse_workflow("jobs:\n  a:\n    steps:\n      - run: x\n").unwrap_err();
        assert!(err.path == "on", "{err}");

        let err = parse_workflow("on:\n  push:\njobs:\n  a:\n    runs-on: x\n    steps: []\n")
            .unwrap_err();
        assert!(err.message.contains("至少需要一个 step"), "{err}");

        // uses+run 互斥
        let err = parse_workflow(
            "on:\n  push:\njobs:\n  a:\n    runs-on: x\n    steps:\n      - uses: actions/checkout@v4\n        run: x\n",
        )
        .unwrap_err();
        assert!(err.message.contains("只能取其一"), "{err}");
    }

    #[test]
    fn matrix_expand_and_render() {
        let wf = parse_workflow(
            "on:\n  push:\njobs:\n  t:\n    runs-on: x\n    strategy:\n      matrix:\n        rust: [stable, nightly]\n        os: [linux, mac]\n    steps:\n      - run: cargo +${{ matrix.rust }} test on ${{ matrix.os }}\n",
        )
        .unwrap();
        let combos = expand_matrix(&wf.jobs[0]);
        assert_eq!(combos.len(), 4, "2x2");
        let (values, suffix) = &combos[3];
        assert!(suffix.contains("rust-nightly"));
        let rendered =
            render_matrix_placeholders(&wf.jobs[0].steps[0].run.clone().unwrap(), values);
        assert!(rendered.contains("cargo +nightly test on mac"));
    }

    #[test]
    fn triggers_push_shorthand_and_manual() {
        let wf =
            parse_workflow("on: push\njobs:\n  a:\n    runs-on: x\n    steps:\n      - run: x\n")
                .unwrap();
        assert_eq!(wf.triggers.push_branches, Vec::<String>::new());
        assert!(!wf.triggers.manual);

        let wf = parse_workflow("on:\n  push:\n  manual: true\njobs:\n  a:\n    runs-on: x\n    steps:\n      - run: x\n").unwrap();
        assert!(wf.triggers.manual);

        let err = parse_workflow("on:\n  schedule:\n    - cron: \"0 * * * *\"\njobs:\n  a:\n    runs-on: x\n    steps:\n      - run: x\n").unwrap_err();
        assert!(err.message.contains("schedule"), "{err}");
    }

    #[test]
    fn rejects_yaml_syntax_error() {
        let err = parse_workflow("jobs: [unclosed\n").unwrap_err();
        assert!(err.message.contains("YAML 语法错误"), "{err}");
    }
}
