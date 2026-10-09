<!-- 由 xtask sync-rules 生成,勿手改;事实来源:AGENTS.md -->

# AGENTS.md — CodePlan AI 协作规则(最高准则)

> 对 AI 编码助手(ZCode / Claude Code / Cursor / Copilot)生效。
> 子目录 `crates/AGENTS.md`、`web/AGENTS.md` 就近叠加生效;冲突时以更深层文件为准,铁律除外。
> 项目背景必读:[PLAN.md](./PLAN.md)(功能基线,71 项带 ID)、[ARCHITECTURE.md](./ARCHITECTURE.md)(架构规约)。

## 常用命令

```bash
cargo build --workspace                # 后端构建
cargo test --workspace                 # 后端测试
cargo clippy --workspace -- -D warnings
cargo xtask lint-deps                  # 依赖解耦检查(CI 同款,加依赖前必跑)
pnpm build && pnpm lint && pnpm test   # 前端
docker compose -f deploy/docker-compose.yml up   # 本地起全套
```

## 铁律(违反 = PR 必拒)

1. **crate 依赖方向**:L1(`common`/`kernel`)← L2(`eventbus`/`storage`/`ext`/`notify`)← L3(业务域 crate)← L4(`server`)。禁止反向;**禁止 L3 域之间互相依赖**。加跨 crate 依赖前跑 `cargo xtask lint-deps`。
2. **跨域协作只有两条路**:领域事件(在 `kernel::events` 定义 schema)或 kernel trait(`Authz`/`IdentityLookup`/`AuditSink`)。禁止 `use` 其他域 crate 的任何类型;禁止 SQL 里查询其他域的表(表名带域前缀,如 `proj_`/`repo_`,前缀即所有权)。
3. **`common` 不准出现业务概念**(User/Project/Issue/PR…),不准依赖本项目任何 crate。业务概念的上沉入口只有 `kernel`。
4. **前端 `features/*` 之间禁止互相 import**;`packages/*` 不得 import `features/*`。
5. Git 写路径只准走 git 官方 `http-backend`/`receive-pack` 代理;读路径只准经 `GitRepo` adapter 走 gix。**禁止引入 git2/libgit2 等 C FFI**。

## 公共代码放置决策树(写公共代码前必读)

先问一个问题:**这段逻辑的第二个真实调用者在哪里?** 回答不出来就不要抽公共。

```
只在当前 crate 内复用
  → 放本 crate 的 support/ 模块(如 crates/project/src/support/)。到此为止,不要动公共层。

2 个以上 crate 需要:
  ├─ 纯工具、无业务语义(分页/时间/字符串/校验/ID 生成/错误包装)
  │    → common/<具名模块>.rs   ← 必须具名(pagination.rs/ids.rs),禁止建 utils.rs 垃圾抽屉
  ├─ 涉及平台契约(基础 ID 类型/权限判定/审计/事件 schema/用户上下文)
  │    → kernel                  ← 全平台耦合点,PR 必须写明影响面
  └─ 是基础设施访问模式(发事件/读写对象存储/搜索索引)
       → 扩展 eventbus/storage/search 的既有接口,不要在业务域里各自封装一份
```

**提升路径只有一个方向**:crate 内 `support/` → (出现第二个真实调用者)→ `common` 或 `kernel`。禁止"以后可能用到"的提前抽象。

判定示例:

- ✅ 分页参数被 `project` 和 `git-service` 同时需要 → 提升到 `common/pagination.rs`
- ✅ "PR 合并后流转工作项" → `code-review` 发 `pr.merged` 事件,`project` 在 `listen/` 订阅消费
- ✅ 多个域需要查"用户是否某项目成员" → 走 `kernel::IdentityLookup` trait,不 import identity 内部
- ❌ `project` 里 `use code_review::PullRequest`(跨域实体引用)
- ❌ 把 `IssueStatus` 枚举放 `common`(业务概念)
- ❌ 新建 `common/utils.rs` "先放放"
- ❌ 为单一调用者写 `pub trait XxxTrait` "方便以后 mock"

## 代码落位

- 新功能先定位:对照 PLAN.md 第 2 节的功能 ID → PLAN.md 3.2 的 crate 列表落位;前端对照 `web/features/*`。拿不准就在 PR 里写明判断依据,不要随意落位。
- 域 crate 内部分层固定:`domain`(纯逻辑,不引 axum/sqlx)/ `infra`(仓储实现)/ `service`(用例+事务边界)/ `api`(路由+DTO+utoipa)/ `listen`(订阅他域事件)。
- 数据库:每个域只读写自己的表;迁移进 `migrations/` 且向后兼容(上一版本二进制必须能跑)。
- API:所有端点必须带 utoipa 注解(OpenAPI→TS client 依赖它);错误码 `<域>-<3位>`(如 `GIT-404`),在所属域的错误枚举注册,不准就地 `anyhow` 返回给前端。
- 事件:先在 `kernel::events` 加 schema(版本化),再发布;订阅方必须幂等(at-least-once)。

## 前端补充(细则见 web/AGENTS.md)

- 组件优先用 `packages/ui` 与 shadcn/ui;**样式值只从 `packages/design-tokens` 出**,禁止硬编码色值/间距/圆角。
- 接口请求只走 `packages/api-client`(生成物,禁止手改);事件类型用 `packages/events`。
- AI 助手/流式/生成式卡片,用 `packages/ai-ui`,不要自己搭。

## AI 生成代码的附加义务

- 生成 UI 前先查 `packages/ui` 与 Storybook 是否已有同类组件——**复用优先于新建**;
- 生成 SQL/迁移前确认表归属域、是否跨域读;
- 提交信息 Conventional Commits;PR 描述必须带功能 ID(如 `PAAS-07`)与改动理由;
- 生成的代码必须当自己的写:过 clippy/tsc、补测试、更新受影响的 AGENTS.md 规则。

## 完成定义(DoD)

`cargo clippy` + `cargo test` + `cargo xtask lint-deps` 通过;涉及端点有 utoipa、涉及事件有 schema、涉及迁移向后兼容;前端 `pnpm lint + test` 且 tsc strict 通过;UI 改动附 Storybook 截图或对照页。
