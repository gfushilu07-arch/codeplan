# crates/ — 后端域规则(叠加根 AGENTS.md)

## 各 crate 职责(落位对照表)

| crate | 只准放 | 禁止出现 |
|---|---|---|
| `common` | 分页/时间/ID 宏/校验/错误包装/日志/config | 业务概念、本项目依赖 |
| `kernel` | 基础 ID 类型、CurrentUser、Authz/AuditSink/IdentityLookup trait、**领域事件 schema(唯一定义处)**、扩展点 re-export、testkit(内存事件总线等测试夹具) | 具体域逻辑、SQL |
| `eventbus` | outbox dispatcher、NATS 封装 | 域语义 |
| `storage` | OpenDAL 封装 | 域语义 |
| `ext` | 插件扩展点 trait、注册与生命周期 | 直接依赖业务域 |
| `project` 等 L3 域 | 本域 domain/infra/service/api/listen | 其他域的类型、其他域的表 |
| `server` | 装配:路由挂载、事件订阅注册、插件注册、OpenAPI 聚合 | 业务逻辑(它是组合根,不是功能域) |

新增 crate:必须能归入 L2/L3/L4 之一,更新 `xtask` 依赖白名单并在 PR 说明归属理由。

## 域内分层纪律

- `domain/` 不引 `axum`、`sqlx`(仓储只定义 trait);实体不 derive utoipa;
- `service/` 划事务边界、发领域事件;一个用例一个函数,禁止 api 层直调仓储;
- `api/` 只做 DTO↔domain 转换 + 权限中间件 + utoipa;DTO 独立定义,不直接返回实体;
- `listen/` 消费他域事件,必须幂等,失败可重试,禁止在 listen 里再发同域事务写(经 service)。

## 领域事件

1. 新事件先在 `kernel::events` 定义(schema 版本化,payload 用强类型 ID);
2. 发布方在 `service` 层随事务写 outbox;禁止绕过 outbox 直发 NATS;
3. 事件命名 `<域>.<动作过去式>`:`pr.merged`、`release.rolled_back`;
4. 改事件 payload = 破坏性变更,需升版本并在 PR 列出全部订阅方。

## Git 层特规(git-service)

- 读路径(浏览/log/diff/blame)只经 `GitRepo` trait → gix adapter;gix 版本在 workspace 锁定,升级 = 单独 PR + 全量回归;
- 写路径(clone/push)只经鉴权后拉起 git `http-backend` CGI;push 后事件由 `post-receive` hook 调内部 API 发出;
- 禁止 git2/libgit2 等 C FFI 依赖(见根铁律 5)。

## 数据库

- 表名域前缀即所有权:`proj_`(project)、`repo_`(git-service)、`rev_`(code-review)、`ci_`、`rel_`(release)、`flg_`(flags)、`app_`(appcenter)、`ins_`(insights)、`ntf_`(notify)、`org_`(kernel/identity);
- 迁移向后兼容:不重命名/不删除列,先加后迁再清(三步走);
- 每张表带 `created_at`/`updated_at`(timestamptz,UTC)。

## 测试

- `domain`/`service` 用 `common::testkit`(内存事件总线 + fixture),不碰真库;
- `infra`/`api` 用 testcontainers-PG 跑集成;
- 修 bug 先写复现测试再修。
