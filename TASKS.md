# CodePlan 开发任务清单

> 配套 [PLAN.md](./PLAN.md)(功能基线)/ [ARCHITECTURE.md](./ARCHITECTURE.md)(代码规约)/ [AGENTS.md](./AGENTS.md)(AI 规则)。
>
> **状态标记**:`[ ]` 未开始 · 标 `🔄` 进行中 · `[x]` 已完成(完成当日记日期)· 标 `⏸️` 受阻(注原因)。
> **规则**:每完成一个任务立即勾选并记日期;任务拆得动就继续拆;新增任务追加编号,不重排已有 ID。
> **每个任务的完成标准** = 根 AGENTS.md 的 DoD(clippy/test/lint-deps 通过、契约完整、前端 strict 通过)。

## 进度总览(每次更新刷新本表)

| 阶段 | 任务数 | 已完成 | 进行中 | 状态 |
|---|---|---|---|---|
| 阶段 0 · 工程基建 | 7 | 7 | 0 | ✅ 完成 |
| 批次一 · 基座 | 20 | 15 | 0 | 🔄 进行中 |
| 批次二 · 协作 | 15 | 0 | 0 | ⬜ 未开始 |
| 批次三 · 流水线 | 18 | 0 | 0 | ⬜ 未开始 |
| 批次四 · 发布与智能 | 18 | 0 | 0 | ⬜ 未开始 |
| **合计** | **78** | **0** | **0** | — |

规模:S < 2 天 · M = 2~5 天 · L > 5 天(单人当量,粗估)

---

## 阶段 0 · 工程基建(任何功能代码之前)

- [x] **T-001** 仓库初始化:git 仓库、根目录骨架(按 ARCHITECTURE.md)、README、Conventional Commits/PR 模板 · S ✅ 2026-10-09
- [x] **T-002** Rust workspace 骨架:19 个 crate 空壳 + xtask + workspace 配置 + clippy/rustfmt 全开 · M ✅ 2026-10-09(cargo check 全绿,rustc 1.99)
- [x] **T-003** xtask:`lint-deps`(crate 依赖白名单检查)+ `sync-rules`(AGENTS.md → cursor/copilot/claude 指针) · M ✅ 2026-10-09(20 crate 通过;违规注入自检 exit=1)
- [ ] **T-004** CI:后端(clippy/test/lint-deps/cargo-deny)+ 前端(lint/test/tsc strict)+ OpenAPI diff 检查 · M
- [x] **T-005** 前端 monorepo 骨架:pnpm + Turborepo + Vite + apps/main + packages/features 空壳 + tooling 共享配置 · M ✅ 2026-10-09(16 包,build/test/lint 全绿)
- [x] **T-006** 本地开发环境:`deploy/docker-compose.yml`(PG/Redis/NATS/MinIO/Nginx)+ 种子数据脚本 · M ✅ 2026-10-09(PG/Redis/NATS healthy;调整:MinIO 官方已撤下 Docker Hub 且 ghcr/quay 本网络不可达 → 移入 `--profile s3` 可选 + 镜像变量覆盖,开发存储走 OpenDAL fs 后端;端口改 5434/6381/4223/8223 避开本机其他项目容器;Nginx 归入 T-131 shell 阶段再加)
- [x] **T-007** 公共管线:utoipa → OpenAPI → TS client 生成;testkit 雏形(内存事件总线 + testcontainers-PG + fixture) · L ✅ 2026-10-09(server 健康端点 + OpenAPI 契约端点;gen:api 生成 api-client;页面经 vite 代理调用打通;kernel::events 事件信封/EventBus trait/内存总线就绪;testcontainers-PG 随 T-101 测试补齐)

**出口**:CI 全绿;`xtask lint-deps` 守住空壳依赖;一条最小链路(一个假端点 → OpenAPI → TS client → 页面调用)跑通。

---

## 批次一 · 基座(功能:ORG/PM/GIT 基础,出口标准见 PLAN.md §5)

### 身份与权限

- [x] **T-101** kernel 契约层:基础强类型 ID(12 个)、CurrentUser、`Authz`/`AuditSink`/`IdentityLookup` trait、common 工具集(ApiResponse/Page/AppError 错误码/ID 宏/时间/axum 适配) · [ORG-03] · L ✅ 2026-10-09(5 个单测过;lint-deps 白名单修正 kernel→common)
- [x] **T-102** identity 域:用户/组织/团队模型 + 本地账号(argon2)+ JWT 会话 + 登录/登出 API · [ORG-01,02] · L ✅ 2026-10-09(org_* 四表迁移;argon2 哈希(jsonwebtoken 10 + aws_lc_rs);bootstrap 管理员;login/logout/me 端到端验证通过:登录→JWT→/api/me 鉴权/401 错误码;6 个单测 + OpenAPI 聚合 + 前端 client 同步)
- [x] **T-103** OIDC 单点登录(openidconnect 4)+ TOTP 两步验证 · [ORG-02] · M ✅ 2026-10-09
  - TOTP:setup(base32 密钥 + otpauth URL)→ enable(验证码确认)→ 登录双阶段(票据 5 分钟换会话)→ disable;±1 周期容差
  - OIDC:环境变量配置化启用(未配置 404),state=签名 JWT 内含 nonce,openidconnect 4 异步模型(泛型 AsyncHttpClient),按邮箱 JIT 开通,成功后 302 回前端
  - 端到端 8 项全过(直连登录/setup/enable/二次验证/关闭/OIDC 禁用分支);迁移 0002 向后兼容
- [x] **T-104** RBAC:系统/项目/自定义角色模型 + Authz 实现 + axum 权限中间件 · [ORG-03] · M ✅ 2026-10-09
  - RbacService 实现 kernel::Authz(平台管理员直通;资源角色 read⊂write⊂admin);ensure() 辅助即权限中间件模式
  - grant/revoke/members/my-role 四端点(管理员校验后操作,角色层级判定)
  - 迁移 0003(org_resource_role);6 个单测 + 端到端 7 项(授予/升级/members/my-role/非法角色 400/撤销)
  - 简化记录:自定义角色引擎延后至项目域需要时引入(当前资源角色三档已覆盖 RBAC 闭环)
- [x] **T-105** LDAP/AD 组织架构与登录对接(ldap3,定时同步) · [ORG-01] · M ✅ 2026-10-09
  - LdapDirectory trait(生产 ldap3 实现 / 测试内存目录);本地无密码或不存在 → 目录认证回退 + JIT upsert(source=ldap)
  - 定时同步(tokio interval,`CODEPLAN_LDAP_SYNC_HOURS`,0 禁用)+ 平台管理员手动触发路由
  - 配置全走环境变量(AD 的 sAMAccountName 等属性名可配);e2e:未启用分支 400/回归登录 ✓;真实 AD 联调需企业环境(已留接口)
- [x] **T-106** 审计:AuditSink 实现(全操作埋点)+ 审计记录查询 API · [ORG-05] · M ✅ 2026-10-09
  - PgAuditSink(org_audit_log 表,迁移 0004)+ kernel::testkit 内存审计槽
  - 埋点:登录成功/失败、TOTP 启用/关闭、RBAC 授予/撤销;审计失败不阻断业务
  - GET /api/admin/audit(平台管理员,limit 1~500);e2e:auth.login 与 rbac.granted 均可查 ✓

### 事件与项目管理

- [x] **T-110** eventbus:outbox 表 + dispatcher + NATS JetStream 封装 + 事件信封;kernel::events 批次一事件(workitem.*/member.*) · [事件体系] · L ✅ 2026-10-09
  - OutboxEventBus 实现 kernel::EventBus(publish=追加 outbox);PgOutboxStore(append/fetch/mark)
  - 中继器 2s 轮询 → NATS 主题 codeplan.events.<type> → 标记 published;失败仅告警下轮重试
  - e2e:outbox 直插事件 → 中继转发并标记(published_at=t);workitem.*/member.* 事件类型随 T-111 域落地时在 kernel::events 定义
- [x] **T-111** project 域:工作项模型(多类型/自定义字段 JSONB)+ 状态机引擎(流转/权限/校验) · [PM-01,02] · L ✅ 2026-10-09
  - 新增 L3 域 crate:codeplan-project(domain/infra/service/api/workflow 五模块)
  - 状态机引擎:内置工作流(story/task/todo→in_progress→in_review→done;bug 含回归回退)+ 项目级 JSON 配置覆盖(proj_workflow 表)
  - 工作项:项目内编号(advisory 锁防并发)、优先级、经办人/报告人、自定义字段(JSONB)
  - 事件闭环:project.created → identity 监听自动授予创建者 admin(修复信封嵌套取值);workitem.created/status_changed 入 outbox
  - 架构:kernel::TokenAuthenticator 契约 + 认证中间件(project 域不依赖 identity);ResourceLocator 上移 kernel
  - e2e 8 项全过:建项目→自动授权→建工作项→非法流转 409→合法流转→列表过滤→按编号取项→事件落 outbox
- [x] **T-112** 迭代 + 里程碑:规划/容量/进度汇总 · [PM-03,04] · M ✅ 2026-10-10
  - proj_sprint/proj_milestone 表 + 工作项关联列(迁移 0007 向后兼容)
  - 迭代:创建(planned)→ 启动(单 active 规则,旧 active 自动回 planned)→ 完成(409 防重复启动);进度(总数/完成数/提交点数 story_points 求和)
  - 里程碑:创建(截止日期 DATE)/列表(进度 total/done)/关闭;工作项关联/移除(PUT)
  - sprint.completed/milestone.closed 事件入 outbox;端到端 8 项全过
- [ ] **T-113** 多视图 API:看板(列配置/拖拽排序)/列表(筛选批量)/树形(需求分解) · [PM-05] · M
- [x] **T-114** 缺陷跟踪类型特化(严重程度/复现步骤/回归) · [PM-08] · S ✅ 2026-10-10
  - BugFields 类型化:severity 枚举(blocker/critical/major/minor/trivial,缺省 major 注入,非法 400)+ repro_steps
  - 门禁:缺陷流转到 resolved 必须已填复现步骤(回归依据,空白不算);update 走规范化合并(不覆盖其他自定义字段)
  - 4 个域单测 + e2e 5 项全过(默认注入/非法 400/门禁 400/补齐后通过/story 回归)
- [ ] **T-115** Wiki:文档树 + comrak 渲染 + 附件(经 storage) · [PM-11] · M
- [x] **T-116** notify 域:站内信 + 事件订阅框架 + 邮件(lettre) · [PM-13] · M ✅ 2026-10-10
  - notify 域:ntf_notification 表(迁移 0010)+ PgNotificationStore + 通知 API(列表/未读数/单条已读/全部已读,仅接收人本人)
  - 事件订阅:workitem.* NATS 通配订阅 → 映射规则(纯函数可测)→ 渠道分发;操作者自身跳过
  - 建项支持 assignee(事件载荷补 assignee/reporter);邮件(lettre)随 IM 渠道批次引入
- [x] **T-117** 插件框架 EXT-01:ext crate 扩展点契约(通知渠道/权限点/前端扩展点)+ **站内通知改造为首个官方插件验证** · [EXT-01] · M ✅ 2026-10-10
  - ext crate:NotificationChannel trait + PluginRegistry(deliver_all 多渠道分发,单渠道失败不影响其他)
  - plugins/inbox 官方内置插件:站内信渠道落 ntf_notification——插件框架由首个插件验证 ✓
  - lint-deps 白名单扩展:plugins/* 目录接入(notify→ext 分发契约、server→插件 白名单化)
  - e2e:建项指派 bob → 事件中继 → 监听映射 → inbox 落库(1s 到账);操作者自身不误发;通知 API 全流程

### Git 基础

- [x] **T-120** git-service:仓库生命周期(创建/删除/元数据/模板)+ 裸仓库管理 · [GIT-01] · M ✅ 2026-10-10
  - 新增 L3 域 crate codeplan-git-service(domain/infra/service/api/storage)
  - repo_repository 元数据表(迁移 0011)+ 裸仓库 git init --bare -b <分支>(<data>/repositories/{owner}/{key}.git)
  - 创建失败回滚(元数据归档);归档语义 = 软删除(DELETE API → archived,404 隐藏,物理保留)
  - repo.created → identity 监听自动授予创建者 repo admin(事件闭环)
  - 模板仓库与导入/镜像延后(T-121 传输层落地后实现);e2e 6 项全过
- [ ] **T-121** Git smart HTTP:鉴权 + `http-backend` CGI 代理 + post-receive hook 事件管道(commit.pushed) · [GIT-02] · L
- [ ] **T-122** 代码浏览:gix 读路径(文件树/README/提交历史/diff/blame)+ GitRepo adapter · [GIT-04] · L

### 平台管理与个人工作台

- [ ] **T-123** 平台管理:系统设置/用户管理/配额 + 备份恢复脚本 + Prometheus 指标/健康检查 · [ADMIN-01~04] · M
- [ ] **T-124** 个人工作台聚合 API(分配我/我创建/@我/待办) · [UX-01] · S

### 前端 · 批次一

- [ ] **T-130** design-tokens 全套(色板/暗色/密度)+ ui 包基础组件(Button/DataTable/StatusBadge/PageHeader/Empty…)+ Storybook · L
- [ ] **T-131** main shell:布局、主导航、路由骨架、登录页、全局 Provider、错误处理 · M
- [ ] **T-132** 工作项页面:看板(dnd-kit 拖拽)/列表/树形 + 工作项详情(状态流转/评论) · L
- [ ] **T-133** Wiki 页面(bytemd 编辑预览)+ 附件管理 · M
- [ ] **T-134** 仓库页面:文件树浏览/提交历史/diff(shiki)/blame · L
- [ ] **T-135** 个人工作台页 + 管理页(用户/组织/审计) · M

**批次一出口自检**:10 人团队跑完一个完整迭代;代码可 clone/push;一个官方插件经扩展点接入成功。

---

## 批次二 · 协作(出口:需求→分支→PR→评审→合并→工作项流转全闭环)

- [ ] **T-201** Git SSH 服务(russh 或 sshd forced-command)+ 仓库级 SSH Key 管理 · [GIT-05 部分, ORG-04] · M
- [ ] **T-202** LFS:batch API + OpenDAL 转存 + git lfs 兼容验证 · [GIT-08] · M
- [ ] **T-203** code-review 域:PR 模型 + 创建/Draft + PR diff 数据 + 合并执行(merge/squash/rebase) · [GIT-09] · L
- [ ] **T-204** 评审流:行内评论(锚定 diff)/通过驳回/状态检查聚合 · [GIT-09] · L
- [ ] **T-205** 分支保护规则引擎(最少审批/状态检查门禁/禁 force push) · [GIT-05] · M
- [ ] **T-206** CODEOWNERS:解析 + 自动指派评审人 + 缺员阻塞 · [GIT-06] · S
- [ ] **T-207** 提交关联:push 事件解析 `#ID` + work_item_ref + PR 合并自动流转(listen)+ 工作项反向展示 · [GIT-10] · M
- [ ] **T-208** Tag/Release:发布 + 附件 + 关联记录自动生成 Release Notes · [GIT-07] · M
- [ ] **T-209** Webhook 出站:事件订阅 + HMAC 签名 + 重试与日志 · [GIT-11] · S
- [ ] **T-210** 访问凭证:GPG/SSH 提交签名验证(sequoia)+ 个人访问令牌(细粒度 scope) · [ORG-04] · M
- [ ] **T-211** IM 卡片交互:统一卡片/回调协议 + 企微适配器(IM 内审批 PR/流转工作项)+ 钉钉/飞书适配器 · [OPEN-02] · L
- [ ] **T-212** 报表引擎:迭代报告/负载/缺陷趋势/吞吐/累计流 + Excel/PDF 导出 · [PM-12] · M
- [ ] **T-213** 全文搜索:tantivy 索引管线(工作项/Wiki)+ 查询 API · [SRCH-01] · M
- [ ] **T-214** 前端:PR 页面(对话流/行内评论/diff 侧栏评论)+ 分支页 + Release 页 · L
- [ ] **T-215** 前端:甘特图 + 需求池 + 报表页 + 导出交互 + Cmd+K 命令面板 · [PM-06,07, UX-02] · M

**批次二出口自检**:全闭环走通;域账号登录;在 IM 里完成一次 PR 审批。

---

## 批次三 · 流水线(出口:提交触发流水线;应用一键部署并经域名健康上线)

### CI/CD 与制品

- [ ] **T-301** workflow YAML 解析(Actions 兼容子集:jobs/steps/uses/run/矩阵)+ 校验器 + 超范围明示报错 · [CI-01] · L
- [ ] **T-302** Runner:bollard 容器调度 + runner 池/标签 + GitHub 官方 runner 二进制复用 · [CI-02] · L
- [ ] **T-303** 流水线执行引擎:调度状态机 + 实时日志回流(SSE)+ 构建缓存卷 · [CI-01,02,05] · L
- [ ] **T-304** 制品库:Maven/npm/PyPI/容器镜像协议 + 产物上传下载 + 流水线产物打通 · [CI-03] · L
- [ ] **T-305** 安全扫描:Semgrep/gitleaks/trivy 容器执行编排 + 结果模型(官方插件)+ PR 门禁卡点 · [CI-04] · M

### 搜索与度量

- [ ] **T-306** 代码搜索:tantivy n-gram 索引 + push 事件增量 + 查询 API 与前端 · [SRCH-02] · M
- [ ] **T-307** DORA:事件聚合事实表 + 四指标计算 + 下钻查询 · [MS-01] · M
- [ ] **T-308** 效能报表(ECharts)+ Excel 导出 + 外部链接器 · [MS-02,03] · M

### 部署引擎底座

- [ ] **T-310** appcenter:应用模型 + 聚合视图 + 环境管理(部署目标登记) · [PAAS-01,02] · M
- [ ] **T-311** agent:Rust 单二进制(tonic gRPC 外连注册/token/心跳)+ 主机管理(DeployHost) · [PAAS-03] · L
- [ ] **T-312** 服务规格:声明式版本化(镜像/端口/环境变量/卷/健康检查/副本数)+ 规格即代码 · [PAAS-03] · M
- [ ] **T-313** 部署执行:期望状态调和 + agent 本地 bollard 拉镜像/起容器 + 健康检查 · [PAAS-04] · L
- [ ] **T-314** 部署可观测:日志/状态实时回流(WebSocket)+ 发布进度可视化数据 · [PAAS-04] · M
- [ ] **T-315** 域名绑定:自动分配/自定义域名 + 网关路由生成(Nginx 配置生成/APISIX Admin API 适配) · [PAAS-05] · M
- [ ] **T-316** 证书插件 cert-acme:ACME 申请(Let's Encrypt/ZeroSSL,泛域名 DNS-01)+ 自动续期 + 部署钩子到网关/LB/主机 · [PAAS-07] · L
- [ ] **T-317** 配置与密钥:AppConfig 版本化 + 敏感项加密存储 + 注入不落明文 · [PAAS-06] · M
- [ ] **T-318** 工时:登记/预估/统计(PM-09) · S

**批次三出口自检**:提交触发流水线、扫描卡住不合规 PR;DORA 出数;应用"一键部署"经绑定域名(证书自动签发)健康上线。

---

## 批次四 · 发布与智能(出口:真实灰度发布全程在平台完成 → V1.0 GA)

### 发布域

- [ ] **T-401** 发布单:模型 + 审批流 + 变更自动汇总(关联 PR/工作项)+ 执行记录 · [PAAS-08] · M
- [ ] **T-402** 灰度策略状态机:滚动/分批/蓝绿 + 批次健康门禁 + 错误率自动停批 · [PAAS-09,14] · L
- [ ] **T-403** 一键回滚:重放上一版本规格 + RollbackRecord · [PAAS-09] · M
- [ ] **T-404** 流量切换:网关权重联动(Nginx upstream/APISIX traffic-split)+ 与灰度批次编排 · [PAAS-10] · M
- [ ] **T-405** 变更日历:排期可视化 + 冲突检测 + 变更审计 · [PAAS-11] · M
- [ ] **T-406** 管控与合规:生产双人复核 + 发布/开关全量审计强化 · [PAAS-14] · M

### 功能开关与实验

- [ ] **T-410** flags 域:规则引擎(用户/部门/白名单/百分比稳定哈希)+ 分发 API + 多语言 SDK(REST) · [PAAS-12] · L
- [ ] **T-411** 开关生命周期:创建→放量→全量→清理回收 + 超期提醒 + 切换审计 · [PAAS-12] · M
- [ ] **T-412** GrowthBook 集成插件:开关联动 + 实验入口与结果页 · [PAAS-13] · M
- [ ] **T-413** 入口认证:应用一键接 SSO(OAuth2 反代) · [PAAS-15] · M
- [ ] **T-414** 日志监控入口:容器日志流查看检索 + 健康面板 + 异常 IM 告警 · [PAAS-16] · M
- [ ] **T-415** 资源与伸缩:CPU/内存限额 + 持久卷 + 手动副本数 · [PAAS-16] · M

### 测试管理与智能

- [ ] **T-420** 测试管理:用例库/测试计划/执行关联缺陷/测试报告 · [PM-10] · L
- [ ] **T-421** MCP Server:工作项/PR/代码/流水线暴露为工具 · [OPEN-03] · M
- [ ] **T-422** AI 助手插件:PR 摘要与风险提示 + 评审初筛 + Issue 分类去重 + 自然语言查迭代(总开关) · [OPEN-04] · L
- [ ] **T-423** 前端 AI UI:聊天侧栏 + 流式消息 + 生成式业务卡片(zod schema ↔ MCP 工具对齐) · [OPEN-04] · M
- [ ] **T-424** IDE 插件:VSCode + JetBrains(工作项/分支/PR/流水线) · [OPEN-05] · M
- [ ] **T-425** 代码片段(内部 Gist)+ 仓库讨论区 + 仓库级 Issue 转工作项 · [GIT-12,13] · M
- [ ] **T-426** 审计合规报表:模板 + PDF/Excel 导出 · [ORG-06] · M
- [ ] **T-427** 前端:发布单/灰度可视化(批次进度/暂停回滚操作台)/变更日历/开关管理页 · L

**批次四出口自检**:真实灰度发布(分批 + 网关切流 + 一键回滚)全程在平台完成;生产开关双人复核与超期回收生效;AI 评审摘要可用 → **V1.0 GA**。

---

## 更新日志(倒序追加)

| 日期 | 完成 | 备注 |
|---|---|---|
| 2026-10-09 | — | 任务清单创建,共 78 项,全部未开始 |
| 2026-10-09 | T-001 | git init(main)+ README + .gitignore + PR 模板;Rust 工具链安装中 |
| 2026-10-09 | T-002 | workspace 19 crate 四层骨架 + xtask 壳;cargo check 全绿(实际 19 crate,较计划 +2:notify/agent 独立) |
| 2026-10-09 | T-003 | lint-deps 白名单检查(含违规注入自检)+ sync-rules 生成 5 个工具规则文件 |
| 2026-10-09 | T-004 | ci.yml(后端六步 + 前端三步)+ deny.toml + rust-toolchain 锁定;fmt/yaml 校验过 |
| 2026-10-09 | T-005 | 前端 monorepo:16 包(apps/main + 7 packages + 7 features),tokens/ui/shared 真实可用,build/test/lint 全绿 |
| 2026-10-09 | T-006 | compose 核心三容器 healthy;MinIO 改可选 profile(镜像源受限),开发存储走 fs 后端;端口避让本机其他容器 |
| 2026-10-09 | T-007 | 最小链路全通:server 健康端点 → OpenAPI → api-client 生成 → 页面代理调用;kernel 事件契约 + testkit 内存总线;阶段 0 完成 ✅ |
| 2026-10-09 | T-101 | common(AppError/ApiResponse/Page/ID 宏/time/axum 适配)+ kernel(ID/CurrentUser/Authz/AuditSink/IdentityLookup)全部带单测;testkit 归入 kernel::testkit |
| 2026-10-09 | T-102 | identity 域落地(domain/infra/service/api 四层);端到端:登录→JWT→鉴权→401 错误码;架构修正:身份域独立为 L3 crate(原计划在 kernel,违反 kernel 禁域逻辑铁律) |
| 2026-10-09 | T-103 | TOTP 双阶段登录 + OIDC 配置化 SSO(JIT 开通);python 脚本端到端 8 项全过;openidconnect 4 异步 API(泛型 AsyncHttpClient/端点状态机)踩平 |
| 2026-10-09 | T-104 | RBAC:Authz 落地 + 资源角色 grant/revoke/members/my-role;端到端 7 项全过;自定义角色引擎按需延后 |
| 2026-10-09 | T-105 | LDAP 目录抽象(生产 ldap3/测试内存双实现)+ 登录回退 JIT + 定时/手动同步;真实 AD 联调留企业环境 |
| 2026-10-09 | T-106 | org_audit_log + PgAuditSink + 五类埋点 + 管理端查询 API;端到端验证 auth.login/rbac.granted 可查 |
| 2026-10-09 | T-110 | outbox 表 + OutboxEventBus + 中继器(2s 轮询→NATS→标记);e2e 直插事件验证转发;server 已接线 event_bus |
| 2026-10-09 | T-111 | project 域落地:状态机引擎/工作项/事件闭环(project.created→自动授权);kernel::TokenAuthenticator 契约;ResourceLocator 上移 kernel;e2e 8 项全过 |
| 2026-10-10 | T-112 | 迭代(单 active 规则/进度汇总含 story_points)+ 里程碑(进度/关闭);工作项关联 PUT 端点;e2e 8 项全过 |
| 2026-10-10 | T-113 | 看板分列视图 + 拖拽重排(工作流校验)+ 需求树形接口;e2e 全过;WIP 限制留待看板配置 |
| 2026-10-10 | T-114 | BugFields 类型化校验(severity 枚举/默认注入/repro_steps 门禁);e2e 5 项全过 |
| 2026-10-10 | T-115 | wiki 域落地:文档树/渲染(comrak GFM)/附件(fs 存储,OpenDAL 接口预留);e2e 9 项全过;修复子树级联删除与 valid_slug 取反 bug |
| 2026-10-10 | T-116/T-117 | notify 域 + ext 扩展点 + plugins/inbox 首个官方插件;e2e:建项指派→事件→分发→落库 1s 到账,操作者跳过规则验证 |
| 2026-10-10 | T-120 | git-service 域落地:仓库生命周期 + 裸仓库 git init + repo.created 自动授权;e2e 6 项全过;模板/导入/镜像随 T-121 后补 |
