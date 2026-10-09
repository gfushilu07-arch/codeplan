# CodePlan 代码架构规约

> 配套 [PLAN.md](./PLAN.md)。本文约定代码仓库结构、解耦规则、公共方法清单、前端技术栈与 UI 设计方向。
> 规划日期:2026-10-09

---

## 1. 后端:Rust workspace 解耦规约

### 1.1 Crate 四层结构与依赖规则(解耦的核心)

```
L4 组合根      server          # 唯一知道所有域的地方:装配路由/事件订阅/插件/聚合 OpenAPI
L3 业务域      project wiki git-service code-review ci release flags appcenter insights ai
L2 基础设施    eventbus storage ext(插件 SDK) notify
L1 公共        kernel(平台契约) common(纯工具)
```

**依赖规则(硬约束,CI 强制检查):**

1. 上层可依赖下层,**禁止反向依赖**;
2. **L3 域与域之间禁止互相依赖**——跨域协作只有两条路:
   - 领域事件:通过 `eventbus` 发布/订阅(NATS + outbox);
   - kernel 中定义的 trait(`Authz`、`IdentityLookup`、`AuditSink` 等),域 crate 只依赖抽象,`server` 装配时注入实现;
3. `common` 纯工具、无业务语义,不依赖本项目任何 crate;
4. `kernel` 只依赖 `common`,不依赖任何域;
5. `plugins/*` 只依赖 `ext + kernel + common`,经扩展点接入,不得 import 域 crate 的内部类型;
6. **强制手段**:`xtask lint-deps`(解析 workspace 各 crate 依赖白名单)+ cargo-deny,CI 卡点——解耦靠机器守,不靠自觉。

### 1.2 域 crate 内部分层(所有域统一目录)

```
crates/project/src/
├─ domain/    # 实体、值对象、领域服务、仓储 trait —— 纯领域,不依赖 axum/sqlx
├─ infra/     # 仓储实现(SQLx/PG)、缓存适配、第三方客户端
├─ service/   # 用例编排、事务边界、领域事件发布
├─ api/       # axum 路由 + DTO + utoipa 注解 + 权限中间件挂载
└─ listen/    # 领域事件订阅者(消费其他域的事件,如"PR 合并 → 流转工作项")
tests/        # 域级集成测试
```

分层规则:`domain` 不引 sqlx/axum(可脱离 HTTP/DB 单测);`service` 持有仓储 trait、划事务边界;`api` 只做协议转换,不写业务。**好处**:业务可测、未来拆微服务时按 crate 整体搬走。

### 1.3 公共方法清单

**`common`(纯工具,无业务语义):**

| 模块 | 内容 |
|---|---|
| response | `ApiResponse<T>`(code/message/data 统一包装)、`Page<T>` / `PageQuery`(page/size/sort/filter) |
| error | `AppError` 体系:thiserror 定义 + **错误码规范**(域前缀 + 三位,如 `GIT-404`)→ HTTP 状态自动映射 → 前端统一处理 |
| id | 强类型 ID 宏 `define_id!`(newtype,防 UserId/RepoId 串用)、雪花 ID/ULID 生成 |
| time | 统一 UTC 封装、时区转换、时间戳格式化 |
| validate | validator 封装 + 常用规则(域名/分支名/邮箱) |
| logging | tracing 初始化、span 助手、trace_id 注入与透传 |
| config | figment 封装(环境变量 > 文件 > 默认)、密钥注入 |

**`kernel`(平台契约):**

| 模块 | 内容 |
|---|---|
| identity | 全平台统一的强类型基础 ID(UserId/OrgId/ProjectId/RepoId/AppId…)与当前用户上下文 `CurrentUser` |
| authz | `Authz` trait(统一权限检查入口)、内置 RBAC 判定实现 |
| audit | `AuditSink` trait + 审计事件模型 |
| events | **领域事件契约(单一事实来源)**:事件信封(id/type/occurred_at/actor/payload)+ 各域事件类型定义,schema 变更走版本化 |
| lookup | `IdentityLookup` trait(域查用户/组织的抽象,避免依赖 identity 内部) |
| ext | 扩展点 trait re-export(插件 SDK 的稳定入口) |
| testkit | 内存事件总线、testcontainers-PG、实体 fixture builder、HTTP 测试客户端 |

### 1.4 API 契约流(前后端类型安全闭环)

各域 `api` 模块写 utoipa 注解 → `server` 聚合导出 OpenAPI → 生成 `web/packages/api-client`(TS client)与 `web/packages/events`(事件 TS 类型)→ 前端零手写接口层。**后端改契约 → CI 检测 TS 类型 diff → 编译期暴露不一致**。

---

## 2. 前端:技术栈与仓库结构

### 2.1 技术栈选型

| 项 | 选型 | 理由 |
|---|---|---|
| 框架 | React 19 + TypeScript(strict) | 生态与招聘面最宽 |
| 构建 | Vite + pnpm + Turborepo | monorepo 增量构建 |
| 组件库 | **shadcn/ui(Radix UI + Tailwind)** | AI 时代首选:主流代码生成工具(v0/Cursor/Claude/Copilot)对 shadcn+Tailwind 的生成质量最高;CSS vars token 化彻底、视觉上限高、暗色模式天然同构;组件源码进仓库,完全可控 |
| 原子样式 | Tailwind CSS 4 | 全站样式基座(shadcn 即建其上),布局/间距/响应式 |
| 数据组件 | TanStack Table v8 + @tanstack/react-virtual | 企业级表格(排序/筛选/分组/虚拟化),补齐 shadcn 缺口,按 shadcn 风格封装进 `ui` 包 |
| 服务端状态 | TanStack Query v5 | 缓存/失效/乐观更新(看板拖拽必需) |
| 客户端状态 | Zustand | 轻,只放 UI 态 |
| 路由 | TanStack Router | 类型安全、嵌套布局、search params 类型化 |
| 图表 | ECharts 5 | DORA/燃尽/趋势,国内报表审美 |
| 拖拽 | dnd-kit | 看板/排序,动效好控 |
| Markdown | bytemd + GFM + mermaid | Wiki/评论编辑预览 |
| 代码高亮 | shiki | 浏览器端 diff/代码渲染,观感现代 |
| 表单 | React Hook Form + zod | zod schema 与后端校验规则对齐 |
| 时间 | dayjs | — |
| AI UI | Vercel AI SDK(useChat/流式原语)+ assistant-ui | AI 助手全局侧栏、SSE 流式渲染、**生成式 UI**(LLM 结构化输出经 zod 约束 → 渲染业务卡片:工作项卡/PR 卡/流水线卡),与后端 MCP 呼应 |
| 组件规范 | Storybook 8 | `ui` 包配套,设计规范即文档;AI 生成组件时把 Storybook 规范喂给模型,保证一致性 |

### 2.2 Monorepo 结构

```
web/
├─ apps/
│  └─ main/               # 应用 shell:布局、路由组装、登录、全局 Provider、命令面板
├─ packages/              # 公共层(对应后端的 common/kernel)
│  ├─ design-tokens/      # 设计令牌:色板/字号/间距/圆角/阴影/暗色映射(AntD theme + CSS vars)
│  ├─ ui/                 # 二次封装组件:PageHeader/DataTable/StatusBadge/Empty/ConfirmModal…(Storybook)
│  ├─ api-client/         # OpenAPI 生成的 TS client(勿手改,CI 校验)
│  ├─ events/             # 领域事件 TS 类型(后端 schema 生成)
│  ├─ shared/             # hooks(useAuthz/usePagination/useDebounce)、utils、错误处理、常量
│  ├─ ai-ui/              # AI 原生组件:聊天侧栏、流式消息、生成式业务卡片(工作项卡/PR 卡/流水线卡)
│  └─ icons/              # 自定义 SVG 图标
├─ features/              # 业务域包(只依赖 packages/*,与后端域一一对应)
│  ├─ workitem/           # 工作项/看板/迭代/测试
│  ├─ repo/               # 仓库/浏览/PR/评审
│  ├─ pipeline/           # 流水线/制品
│  ├─ deploy/             # 应用/发布/灰度/开关
│  ├─ insights/           # DORA/报表
│  ├─ search/
│  └─ admin/
└─ tooling/               # 共享 eslint/tsconfig/prettier 配置
```

**前端解耦规则(与后端同构):** `features` 之间禁止互相 import;跨域跳转只走路由与事件;`main` 是前端组合根。`packages/*` 不得 import `features/*`。

### 2.3 UI 设计方向(参考 Gitee / GitCode,"好看"的具体抓手)

**参考定位:** 信息架构学 **Gitee**(企业研发工具的导航与功能布局),视觉质感学 **GitCode / GitHub 新版**(留白、层次、现代感),整体气质:专业、克制、开发者友好。

**信息架构:**
- 顶部主导航:工作台 / 项目 / 代码 / 流水线 / 制品 / 部署 / 效能 / 管理;
- 项目内二级导航(对标 Gitee 项目棚):概览 / 工作项 / 迭代 / Wiki / 测试 / 设置;
- 应用详情页三级导航(对标云效):代码 / 流水线 / 环境 / 发布 / 开关 / 监控。

**Design Tokens(所有颜色/字号/间距只从 tokens 出,`packages/design-tokens` 单一来源):**
- 主色:靛蓝/深蓝系(与 Gitee 亮蓝区分,更沉稳),完整语义色(成功/警告/危险/链接);
- 中性色 10 阶;**暗色模式一级公民**(shadcn CSS vars 双主题,一键切换);
- 字体:系统栈 + 中文优化(PingFang SC / Microsoft YaHei fallback);代码/ID/日志统一等宽(JetBrains Mono → 系统 mono);
- 8px 间距网格、圆角 6px、三级阴影、两套密度(紧凑默认/舒适可选——开发者工具默认紧凑)。

**质感细节(区别于"默认 AntD 后台"的关键):**
- 骨架屏统一、页面切换与看板拖拽动效(dnd-kit)、Cmd+K 命令面板、统一 toast/空状态插画与新手引导;
- diff 视图精心排版(等宽、行高、折叠、侧栏评论锚点)——代码场景是颜值主战场;
- 图表统一主题(ECharts 注册同一套 token)。

**质量门槛:** `ui` 包 Storybook 覆盖全部基础组件;关键页面(工作台/看板/仓库/PR/diff/发布单)对照 Gitee/GitCode 逐页 walk-through 评审;首屏性能预算 < 2s。

---

## 3. 工程规约(两端通用)

- **后端**:clippy 全开、rustfmt、cargo-deny(依赖审计 + crate 依赖白名单)、domain/service 层测试覆盖率门槛;
- **前端**:eslint + prettier、tsc strict、禁 any、hooks 规范;
- **提交**:Conventional Commits(PR 标题自动校验)、PR 必须关联工作项 ID(`#ID` 自家语法);
- **自举(dogfooding)**:CodePlan 项目自身的仓库托管在 CodePlan 上,自己的研发流程(工作项 → 分支 → PR → CI → 灰度发布)全程走平台——团队即第一批真实用户,发布前每个功能都被自己用过。

---

## 4. AI 协作规则(AI Rules)体系

> AI 编码助手是本项目的第一类贡献者。规则体系与代码同权重设计。

### 4.1 文件布局(单一事实来源 + 就近生效)

```
AGENTS.md            根:命令、五条铁律、公共代码放置决策树、落位规则、DoD
crates/AGENTS.md     后端细则:crate 职责对照表、域内分层纪律、事件/迁移/测试规约、Git 层特规
web/AGENTS.md        前端细则:结构依赖、前端版放置决策树、UI 规范、AI 原生 UI、质量门槛
```

- `AGENTS.md` 是开放标准(ZCode/Claude Code/Cursor/Copilot 均原生或经配置支持),子目录文件就近叠加生效——AI 改哪块代码就读哪块的规则;
- **工具专属文件不单独维护**:脚手架提供 `xtask sync-rules`,从 AGENTS.md 生成 `.cursor/rules`、`.github/copilot-instructions.md`、`CLAUDE.md` 指针,保证单一事实来源。

### 4.2 书写原则(为什么这么写)

1. **机器管的写进 CI,规则只管机器管不了的判断**:依赖方向有 `xtask lint-deps` 强制,规则里只放一句引用;“公共代码放哪”是判断题,才是规则的正文——决策树 + 正反例(✅/❌)是 AI 执行最稳定的形式;
2. **每条规则带一行 why**(如"表名域前缀即所有权"):无理由的禁令会被 AI 机械执行出岔子;
3. **默认不抽象**:公共化的唯一路径是"crate 内 support/ → 出现第二个真实调用者 → 提升 common/kernel",专治 utils 垃圾抽屉和过早抽象;
4. **短**:根文件 ~100 行,细则文件 ~60 行;规则超 150 行就该拆或删。

### 4.3 维护纪律

- AGENTS.md 与代码同权重:改动进 code review;架构决策变更(新增 crate、新扩展点、新公共层)必须同步更新对应规则文件,否则 PR 不合入;
- 规则过期即删;发现 AI 反复犯同类错 → 优先把它变成 lint/xtask 检查(机器守),实在守不住才加规则条款;
- 每季度回顾一次:哪些规则从未被违反(可降为注释)、哪些错误没被覆盖。
