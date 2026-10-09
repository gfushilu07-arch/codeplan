# CodePlan

企业内部一体化研发平台:**项目管理 + 代码托管 + 发布交付**(内部 PaaS + 研发管理闭环)。

- 项目管理:对标 Jira/PingCode 的中重型研发闭环(工作项/迭代/看板/测试)
- 代码管理:对标 GitHub 核心能力(仓库/PR 评审/分支保护)
- 发布交付:自研轻量部署引擎(agent + Docker,无 K8s)、灰度发布、功能开关、域名绑定
- 技术栈:Rust(axum + SQLx)+ React(shadcn/ui + Tailwind)

## 文档

| 文档 | 内容 |
|---|---|
| [PLAN.md](./PLAN.md) | 产品规划:71 项功能基线、技术架构、四批次路线 |
| [ARCHITECTURE.md](./ARCHITECTURE.md) | 代码架构规约:crate 分层解耦、公共方法、前端栈与 UI 方向 |
| [AGENTS.md](./AGENTS.md) | AI/人类协作规则:铁律、公共代码决策树、DoD |
| [TASKS.md](./TASKS.md) | 开发任务清单(78 项,含进度标记) |

## 仓库结构

```
crates/     Rust workspace(19 个 crate,四层:公共/基础设施/业务域/组合根)
plugins/    官方内置插件(证书自动申请、通知渠道、扫描器…)
web/        前端 monorepo(apps/main + packages/* + features/*)
deploy/     docker-compose、Nginx 模板、备份脚本
docs/       设计文档
xtask/      仓库工程任务(lint-deps / sync-rules)
```

## 本地开发

```bash
# 起依赖(PG/Redis/NATS/MinIO)
docker compose -f deploy/docker-compose.yml up -d

# 后端
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings

# 前端
pnpm install
pnpm build && pnpm test
```

> 提交遵循 Conventional Commits;PR 必须关联工作项 ID(`#ID`)。
