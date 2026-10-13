# CodePlan 开发任务清单

> 配套 [PLAN.md](./PLAN.md)(功能基线)/ [ARCHITECTURE.md](./ARCHITECTURE.md)(代码规约)/ [AGENTS.md](./AGENTS.md)(AI 规则)。
>
> **状态标记**:`[ ]` 未开始 · 标 `🔄` 进行中 · `[x]` 已完成(完成当日记日期)· 标 `⏸️` 受阻(注原因)。
> **规则**:每完成一个任务立即勾选并记日期;任务拆得动就继续拆;新增任务追加编号,不重排已有 ID。
> **每个任务的完成标准** = 根 AGENTS.md 的 DoD(clippy/test/lint-deps 通过、契约完整、前端 strict 通过)。

## 进度总览(每次更新刷新本表)

| 阶段 | 任务数 | 已完成 | 进行中 | 状态 |
|---|---|---|---|---|
| 阶段 0 · 工程基建 | 7 | 7 | 0 | ✅ 完成(T-004 OpenAPI diff 检查未纳入,见该条偏差) |
| 批次一 · 基座 | 25 | 25 | 0 | ✅ 完成 |
| 批次二 · 协作 | 16 | 16 | 0 | ✅ 完成 |
| 批次三 · 流水线 | 17 | 16 | 0 | 🔄 进行中(余 1 项) |
| 批次四 · 发布与智能 | 20 | 0 | 0 | ⬜ 未开始 |
| **合计** | **85** | **64** | **0** | — |

> 2026-10-11 校准:补勾 T-004/T-113/T-115/T-121/T-122(代码与更新日志早已完成,仅复选框漏勾);
> 修正批次二任务数 15→16(含 T-216)、批次三已完成 5→4。三处已交付但范围收窄的偏差见对应任务条目。

规模:S < 2 天 · M = 2~5 天 · L > 5 天(单人当量,粗估)

---

## 阶段 0 · 工程基建(任何功能代码之前)

- [x] **T-001** 仓库初始化:git 仓库、根目录骨架(按 ARCHITECTURE.md)、README、Conventional Commits/PR 模板 · S ✅ 2026-10-09
- [x] **T-002** Rust workspace 骨架:19 个 crate 空壳 + xtask + workspace 配置 + clippy/rustfmt 全开 · M ✅ 2026-10-09(cargo check 全绿,rustc 1.99)
- [x] **T-003** xtask:`lint-deps`(crate 依赖白名单检查)+ `sync-rules`(AGENTS.md → cursor/copilot/claude 指针) · M ✅ 2026-10-09(20 crate 通过;违规注入自检 exit=1)
- [x] **T-004** CI:后端(clippy/test/lint-deps/cargo-deny)+ 前端(lint/test/tsc strict)+ OpenAPI diff 检查 · M ✅ 2026-10-09
  - `deploy`:`ci.yml`(后端六步:fmt/clippy/test/lint-deps/sync-rules/cargo-deny;前端三步:lint/test/build)+ `deny.toml` + rust-toolchain 锁定
  - 偏差:**OpenAPI diff 检查未纳入**(标题含此项但实现为六步);如需补,可在前端步骤后加 utoipa 导出与基线比对
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
- [x] **T-113** 多视图 API:看板(列配置/拖拽排序)/列表(筛选批量)/树形(需求分解) · [PM-05] · M ✅ 2026-10-10
  - 看板分列视图 `/api/projects/{key}/board` + 拖拽重排 `board/reorder`(工作流合法性校验,非法流转 409);需求树形 `/api/projects/{key}/items/tree`
  - 偏差:**列表视图(筛选/批量操作)未实现**;WIP 限制留待看板列配置
- [x] **T-114** 缺陷跟踪类型特化(严重程度/复现步骤/回归) · [PM-08] · S ✅ 2026-10-10
  - BugFields 类型化:severity 枚举(blocker/critical/major/minor/trivial,缺省 major 注入,非法 400)+ repro_steps
  - 门禁:缺陷流转到 resolved 必须已填复现步骤(回归依据,空白不算);update 走规范化合并(不覆盖其他自定义字段)
  - 4 个域单测 + e2e 5 项全过(默认注入/非法 400/门禁 400/补齐后通过/story 回归)
- [x] **T-115** Wiki:文档树 + comrak 渲染 + 附件(经 storage) · [PM-11] · M ✅ 2026-10-10
  - wiki 域落地:文档树/渲染(comrak GFM)/附件(fs 存储,OpenDAL 接口预留);e2e 9 项全过;修复子树级联删除与 valid_slug 取反 bug
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
- [x] **T-121** Git smart HTTP:鉴权 + `http-backend` CGI 代理 + post-receive hook 事件管道(commit.pushed) · [GIT-02] · L ✅ 2026-10-10
  - smart HTTP 代理 + Basic 鉴权 + post-receive 钩子事件管道;真实 `git clone`/`push` 往返 e2e 全过(含无凭据 401)
- [x] **T-122** 代码浏览:gix 读路径(文件树/README/提交历史/diff/blame)+ GitRepo adapter · [GIT-04] · L ✅ 2026-10-10
  - GitBrowse adapter(**CLI porcelain 实现**)+ 6 个浏览端点;e2e 7 项全过;blame 直接用 porcelain 无需朴素算法
  - 偏差:读路径走 **git CLI 而非 gix**(`GitRepoLookup` trait 已抽象,`CliGitAccess` 为当前实现);性能敏感时按接口换 gix 实现即可,不阻塞功能

### 平台管理与个人工作台

- [x] **T-123** 平台管理:系统设置/用户管理/配额 + 备份恢复脚本 + Prometheus 指标/健康检查 · [ADMIN-01~04] · M ✅ 2026-10-10
  - 平台管理员用户列表/禁用启用(防自禁拦截);/api/dashboard/counts;/api/metrics/summary;/api/health(已有)
  - deploy/backup.sh:PG 逻辑备份 + 仓库/附件 tar(实测产出备份包);配额简化为仓库/应用计数上限随批次二补
- [x] **T-124** 个人工作台聚合 API · [UX-01] · S ✅ 2026-10-10
  - /api/dashboard/my-items(assignee/reporter 跨项目聚合,updated_at 倒序);未读数经 notify API 前端聚合
  - e2e 7 项全过(用户列表/禁用自己拦截/无令牌 401/计数/my-items/metrics)

### 前端 · 批次一

- [x] **T-130** design-tokens 全套(色板/暗色/密度)+ ui 包基础组件 + api-client 认证封装 · L ✅ 2026-10-10
  - design-tokens:品牌靛蓝/语义色/中性阶/字体(中文优化+等宽)/8px 间距/圆角阴影/暗色映射(CSS vars)
  - ui:Button(primary/secondary/danger,token 化);shared:cn/错误码校验;api-client:login/fetchMe/my-items + 令牌存取
  - Storybook 随批次二组件扩充时引入(v1 组件量少,以 main 页面实测代替组件文档)
- [x] **T-131** main shell:顶部主导航(Gitee 式信息架构)+ 登录页(JWT 会话)+ 哈希路由骨架 · M ✅ 2026-10-10
  - vite 代理 /api → 后端(CODEPLAN_BACKEND_URL 可配,默认 8081)
  - fetchMe 会话恢复/登出/错误提示;e2e:登录→shell→工作台数据经代理返回 ✓
- [x] **T-132** 工作项页面:看板(dnd-kit 拖拽)/列表/树形 + 工作项详情(状态流转/评论) · L ✅ 2026-10-12
  - 后端:评论域(迁移 0026 `proj_work_item_comment` + `CommentRepository`/`PgCommentRepository`/service 用例,含作者或写权限的删除判定 + `workitem.commented` 事件)
  - 后端:新增 `GET /api/workitems/{id}`(详情全字段)、`GET /api/workitems/{id}/transitions`(可流转目标由**服务端状态机**裁定,避免前后端漂移)、评论增删查三端点;`ItemResponse` 补 `created_at`/`updated_at`
  - 前端:`WorkItemDetail`(流转按钮 + 描述 + 关联提交 + 评论对话流 + 属性侧栏)、`WorkItemListView`(项目/类型/状态/关键词筛选);路由 `#/items`、`#/item/{id}`;看板卡片与需求池行加入详情入口
  - 修 api-client 契约错位:`ProjectItem` 曾声明 `item_number`/`created_at`/`updated_at` 而后端实际返回 `number` 且无时间字段(该类型此前无人消费,故未暴露)→ 两侧对齐
  - e2e:后端 8 项(详情/流转/评论增删/空正文 400/无令牌 401)全通;浏览器实测列表 25 行→筛选(类型 4 行、组合 0 行)→详情→发表评论→流转 进行中→评审中(按钮随之变为「→ 进行中 / → 已完成」)→刷新持久化→删除评论;看板/需求池入口均可达详情
- [x] **T-133** Wiki 页面:文档树 + 编辑(Markdown)/渲染 + 附件入口 · M ✅ 2026-10-10
  - features/wiki:树侧栏(嵌套渲染/新建)+ 页面查看(服务端 HTML)/编辑(前端简渲染预览,保存后以服务端为准)+ 删除(级联)
  - api-client:wikiTree/fetchWikiPage/create/update/delete/uploadAttachment
  - e2e 经 vite 代理:建页+取页(HTML 渲染正确)✓;附件上传下载 UI 随批次二补(后端/接口已备)
- [x] **T-134** 仓库页面:文件树浏览/提交历史/diff(shiki)/blame · L ✅ 2026-10-10
  - features/repo:文件/提交双 Tab,文件树点击预览 blob(等宽字体),提交列表(OID 截断/作者/日期),克隆地址展示
  - api-client:repoTree/repoCommits/repoBlob;diff(shiki 高亮)/blame UI 随批次二(后端 API 已备)
  - e2e 经 vite 代理:树/历史/接口全通;build/lint/test 全绿
- [x] **T-135** 个人工作台页 + 管理页(用户/组织/审计) · M ✅ 2026-10-12
  - 个人工作台页此前已交付(发现页布局);本次补齐管理页三标签(`AdminPage`:用户/组织/审计日志)
  - 组织架构(ORG-01):`OrgRepository` 原已实现但**从未接线**(无 service/API/组合根,团队表为空)→ 扩展为组织+团队+成员仓储并接入 `AuthState`
  - 新增 7 个平台管理员端点:`GET/POST /api/admin/orgs`、`GET/POST /api/admin/orgs/{id}/teams`、`GET/POST /api/admin/teams/{id}/members`、`DELETE /api/admin/teams/{id}/members/{user_id}`;成员按登录名添加,列表回填用户名/显示名
  - 前端:`AdminOrgsPage`(组织→团队→成员三级维护,含新建/移出/添加)、`AdminAuditPage`(时间/操作者/动作/资源/详情,动作着色 + 关键词筛选 + 100/200/500 条)
  - e2e:建组织→重名 409→建团队→按登录名加成员→成员列表回填→组织列表含团队数→不存在用户 404;**非管理员 7 个端点全 403、未认证 401**;浏览器实测三标签切换、审计 82 条 + 筛选 77 条、界面新建团队成功
  - **批次一出口标准达成**:至此批次一 25/25 全部完成(10 人团队可跑完整迭代、代码可 clone/push、官方插件经扩展点接入)

**批次一出口自检**:10 人团队跑完一个完整迭代;代码可 clone/push;一个官方插件经扩展点接入成功。

---

## 批次二 · 协作(出口:需求→分支→PR→评审→合并→工作项流转全闭环)

- [x] **T-201** Git SSH 服务(russh 或 sshd forced-command)+ 仓库级 SSH Key 管理 · [GIT-05 部分, ORG-04] · M ✅ 2026-10-10
  - russh 0.64 内置 SSH 服务(端口 2322,避开 Docker 占用的 2222);ed25519 主机 key 由 ssh-keygen 自动生成并持久化
  - 公钥认证:指纹 → kernel SshKeyDirectory → identity SSH Key 表查用户;密钥 CRUD API 已有
  - exec 通道解析 `git-upload-pack/receive-pack '/owner/name.git'` → 仓库查找 + 读/写权限校验 → `git upload-pack/receive-pack` 子进程全双工管道(stdout 泵 → 通道,data 回调 → mpsc 桥 → stdin)
  - 客户端半关闭(EOF)→ 关闭 git stdin;子进程退出 → exit-status + 通道关闭(客户端拿到正确退出码)
  - e2e 验证:真实 `git clone` + `git push` 走 `ssh://git@localhost:2322/` 全通,提交落盘正确
- [x] **T-202** LFS:batch API + OpenDAL 转存 + git lfs 兼容验证 · [GIT-08] · M ✅ 2026-10-10
  - storage crate 落地:ObjectStore(opendal 0.59 fs 后端,put/get/size/delete + 单测)
  - LFS basic transfer:batch(签发带短时 HMAC 令牌的 href)/ upload(sha256 校验后转存 `sha256/{oid}`)/ download / verify;挂在 smart HTTP 代理下 `info/lfs/*`,CGI 前分流
  - 双认证:Basic(JWT + RBAC)与 `RemoteAuth` 短时令牌(绑定用户+仓库+操作,24h)
  - SSH:`git-lfs-authenticate` 端点(校验权限后频发令牌);git-lfs 3.7 的 git-lfs-transfer 尝试自动回退此路径
  - e2e:真实 git-lfs 5MB 对象 push/clone 全通(HTTP 与 SSH 双路径,sha256 一致;仓库内为指针文件)
- [x] **T-203** code-review 域:PR 模型 + 创建/Draft + PR diff 数据 + 合并执行(merge/squash/rebase) · [GIT-09] · L ✅ 2026-10-10
  - 解耦:kernel 新增 `git` 契约(GitRepoLookup/GitAccess,含 merge-tree/commit-tree/cherry-pick 服务端合并),git-service `adapter.rs` 用 CLI 实现,server 装配注入(遵守 L3 域间禁依赖)
  - rev_pull_request 表(仓库内 number 并发安全:advisory lock + max+1);PrState 状态机(open/merged/closed + draft)
  - API 8 端点:创建(校验分支存在/有差异,信封统一)/ 列表(状态过滤)/ 详情 / diff(merge-base 三点补丁)/ commits / 编辑 / 关闭重开 / 合并
  - 合并三方式实测全通:merge(双亲合并提交)/ squash(压缩单亲,内容校验一致)/ rebase(临时 worktree cherry-pick 重放,线性历史);draft 门禁、终态守卫、冲突 409(附冲突文件)、UpToDate 拒绝、CAS 防并发前移
  - 事件:pr.created/updated/merged/closed/reopened 经 outbox 落库;单测 8 项(编排逻辑,假 GitAccess)
- [x] **T-204** 评审流:行内评论(锚定 diff)/通过驳回/状态检查聚合 · [GIT-09] · L ✅ 2026-10-10
  - 三张表:`rev_pr_comment`(对话流 + 行内锚定 file/line/side/anchor_sha)、`rev_pr_review`(评审历史,聚合取每人最新)、`rev_commit_status`(repo+sha+context upsert)
  - ReviewService:行内评论校验(文件必须在当前 diff、行号>0)、评审规则(自评通过/驳回拒绝、仅开放 PR 可评、历史保留)、状态聚合(none/pending/failure/success)
  - API 7 端点:评论增删查 / 评审提交与列表 / review-summary / PR 状态聚合 / 任意提交状态上报与查询
  - e2e:双用户评审(自评 400、通过→聚合 approved、改驳回→changes_requested)、行内锚定(diff 外 400)、CI 上报 pending→success+failure→combined failure、事件 pr.comment_created/pr.review_posted/commit.status_updated 全落库;单测 9 项
- [x] **T-205** 分支保护规则引擎(最少审批/状态检查门禁/禁 force push) · [GIT-05] · M ✅ 2026-10-10
  - `rev_branch_protection` 表(分支模式精确/`*` 前缀通配、min_approvals、required_contexts、禁 force、禁直推);规则管理 API(Admin 权限,upsert/list/delete)
  - 合并门禁:`PullRequestService::merge` 前置校验,复用 T-204 评审聚合(通过人数/结论)+ 状态聚合(必需 context 必须 success),违规 409 附明细
  - 推送门禁:裸仓库 **pre-receive 钩子**(新建仓库自动安装)→ 平台内部端点 → kernel `PushGuard` 契约(code-review `RulePushGuard` 实现);force 判定因 pre-receive 对象隔离放在钩子进程本地(平台定策略返回待查 ref,钩子 `merge-base --is-ancestor` 执行),HTTP/SSH 双传输统一生效
  - e2e:直推保护分支拒绝(原因回显推送者)、无规则放行、合并门禁三态(全缺/CI 成功无评审/通过后放行)、FF 推送放行、amend 力推拒绝、删除保护分支拒绝、解除保护后力推放行
  - 修潜伏 bug:钩子 `GIT_DIR` 是相对路径导致仓库解析为空(post-receive 事件从未发出)→ 改 `$(pwd)`;解析失败 fail-open 并记日志;单测 3 项(模式匹配/合并门禁两态)
- [x] **T-206** CODEOWNERS:解析 + 自动指派评审人 + 缺员阻塞 · [GIT-06] · S ✅ 2026-10-10
  - 解析:`owners.rs` 纯函数(精确/`前缀*`/`*.后缀`/`目录/` 四种模式,最后命中生效,查找顺序根 → docs/ → .github/ 与 GitHub 一致)
  - 自动指派:PR 创建时按变更文件匹配所有者 → `rev_pr_reviewer` 指派(排除作者、平台不存在跳过、上限 10 人);reviewers API 支持人工增删与当前结论展示
  - 缺员阻塞:保护规则新增 `require_code_owner` → 合并门禁校验每个有所有者的变更文件至少一名所有者 approved(全部所有者不在平台视为缺员)
  - kernel 扩展:`GitAccess::file_content`(读 CODEOWNERS)+ `IdentityLookup::by_username`(identity 新增 PgIdentityLookup 实现)
  - e2e:codeowners 端点解析、建 PR 自动指派 reviewer、未评审 409、所有者 approve 后 squash 合并 OK;单测 7 项(解析/覆盖聚合/自动指派/缺员门禁)
- [x] **T-207** 提交关联:push 事件解析 `#ID` + work_item_ref + PR 合并自动流转(listen)+ 工作项反向展示 · [GIT-10] · M ✅ 2026-10-10
  - 引用约定:`#项目KEY-编号`(如 `fix #CL-1`),大小写不敏感
  - git-service:publish_push 提取各引用新增提交(old..new,新建取顶端 50 条)带完整消息进 `commit.pushed` payload
  - project::listen(RefLinker):订阅 commit.pushed → 解析 `#KEY-N` → `proj_work_item_commit` 建链(幂等);订阅 pr.merged → 合入默认分支时标题/描述/合并提交中的引用自动流转(bug→resolved,其余→done)
  - 状态机新增 `path()`(BFS 最短合法路径);`transition_system` 沿路径逐步流转(事件 actor=system);系统流转豁免缺陷复现步骤门禁(T-114 门禁只管人工)
  - 反向展示:`GET /api/workitems/{id}/commits`;`ProjectService::transition_system` 可复用
  - e2e:推送 `fix #CL-1` 自动建链(2 条)→ 反向展示可见;PR 合入 main → bug 自动 open→in_progress→resolved;事件 workitem.status_changed actor=system 落库
- [x] **T-208** Tag/Release:发布 + 附件 + 关联记录自动生成 Release Notes · [GIT-07] · M ✅ 2026-10-10
  - kernel GitAccess 扩展:`create_tag/delete_tag/list_tags`(附注标签,for-each-ref 剥离到 commit);`commits_between` 支持空 base = 全量历史(上限 100)
  - `repo_release`/`repo_release_asset` 两表;发布 upsert 语义(同标签重发布即更新)
  - 自动 Notes:上一标签以来提交列表(`- 主题 (\`短sha\`)`);上一标签**拓扑探测**(指向提交为当前 tag 祖先且提交时间最新 —— creatordate 排序对同批标签不稳定);可显式传 previous_tag/body 覆盖
  - 附件:OpenDAL 对象存储 `release/{id}/{filename}`(512MB 上限,文件名白名单),上传/下载/删除随 Release 级联清理
  - API 9 端点(tags 3 + releases 4 + assets 2 上下载删);事件 `release.published`
  - e2e:打 tag→发布自动 Notes(区间正确)→附件上传/列表/下载/删除全通;单测 5 项(Notes 生成/文件名校验)
- [x] **T-209** Webhook 出站:事件订阅 + HMAC 签名 + 重试与日志 · [GIT-11] · S ✅ 2026-10-10
  - `repo_webhook`(URL/HMAC 密钥/订阅事件类型 TEXT[] 空=全部/active)+ `repo_webhook_delivery` 投递日志
  - 投递 worker:订阅 `codeplan.events.>`,按 payload repo_id 匹配仓库订阅,HMAC-SHA256 签名(`X-Codeplan-Signature: sha256=…`)+ 事件/投递 ID 头;失败重试 3 次(1s/2s 退避,单次 10s 超时),终态落日志
  - API 5 端点:注册(secret 缺省自动生成)/列表(密钥脱敏)/删除/测试投递(ping)/投递日志
  - e2e:真实 Python 接收器验签(ping 与 release.published 签名均通过);死端口投递 attempts=3 且错误落日志;单测 3 项(签名/匹配/payload 提取)
- [x] **T-210** 访问凭证:GPG/SSH 提交签名验证(sequoia)+ 个人访问令牌(细粒度 scope) · [ORG-04] · M ✅ 2026-10-10
  - PAT:`org_personal_access_token`(只存 SHA-256,明文仅签发时返回一次);scopes(api/repo:read/repo:write);JWT 会话 scopes=["*"];`CurrentUser.has_scope` + smart HTTP/LFS 双处门禁;签发/列表/吊销 API;verify_token 按 `cpat_` 前缀分流
  - 签名验证:kernel `GitAccess::raw_commit`(不经 trim,保签名 payload 完整);`split_gpgsig` 按行解析 gpgsig 头(continuation 行剥前导空格,consumed 按原始行计)
  - GPG:sequoia-openpgp 2.4(纯 Rust crypto + allow-experimental/variable-time feature);StandardPolicy 密钥验证 + 提交者邮箱归属匹配;`org_user_gpg_key` 表(armor 存库)
  - SSH:`ssh-keygen -Y verify` 动态构建 allowed_signers(principal = 提交者邮箱,公钥来自平台 SSH Key)
  - API:`POST /api/repos/{o}/{k}/signing/verify`;事件无
  - e2e:SSH 签名提交 verified=true(未签名对照 method=none);GPG 签名提交 verified=true + OpenPGP 指纹 + signer 归属平台用户;PAT 只读 403/读写 200(curl 直测);排障 3 连:split 偏移差 1、consumed 剥空格差值、cat-file trim 掉签名必需的尾换行
- [x] **T-211** IM 卡片交互:统一卡片/回调协议 + 企微适配器(IM 内审批 PR/流转工作项)+ 钉钉/飞书适配器 · [OPEN-02] · L ✅ 2026-10-10
  - 统一卡片协议(notify::im,L2 纯函数):Card{title,body,actions[]} → 三平台渲染(企微 markdown / 钉钉 actionCard / 飞书 interactive)
  - 回调解析:三平台 JSON → 统一 ImCallback{platform_user, action_id}(钉钉 senderStaffId+actionURL、飞书 open_id+action.value、企微 FromUserName+action_id)
  - 动作协议:`pr:approve|reject:{repo}:{number}`(IM 内审批 PR)、`wi:transition:{item_id}:{to}`(IM 内流转工作项);动作执行经 BoxFuture handler(组合根注入 ReviewService/ProjectService,以映射用户身份执行,守全部领域规则)
  - 身份映射:`CODEPLAN_IM_USER_MAP`(平台用户标识 → 平台用户名 JSON),未绑定 → denied
  - 回调端点:`POST /api/im/{provider}/callback`(wecom/dingtalk/feishu)
  - e2e:钉钉回调 reviewer 通过 PR#9(聚合变 approved)、未绑定用户 denied、企微回调 admin 流转 bug open→…→closed(非法目标被状态机拒)、未知动作 unknown、未知平台 404;渲染/解析单测 6 项
- [x] **T-212** 报表引擎:迭代报告/负载/缺陷趋势/吞吐/累计流 + Excel/PDF 导出 · [PM-12] · M ✅ 2026-10-10
  - insights 域落地(横切分析:proj_* 只读投影 SQL 直查,写入权仍归各域):计算纯函数 + 单测
  - 五报表:迭代报告(总数/完成/故事点/完成率)、负载(按经办人未完成项+点数降序)、缺陷趋势(近 N 天创建/完结日桶补零)、吞吐(近 N 周 ISO 周一桶)、累计流(累计新建/完结/在制品)
  - 导出:Excel(rust_xlsxwriter,PK zip 校验)+ PDF(printpdf 0.12 Op API,系统 TTF 内嵌 PingFang/Noto 支持中文);5 类报表 × 2 格式统一入口 `/reports/export/{kind}`
  - API 6 端点;e2e:RPT 项目造数(迭代 3 项/2 done/9 点)→ 迭代报表 total=3/points 8/9/ratio 67、负载、趋势、吞吐本周=2、累计流 created=4/done=2/wip=2;xlsx 5.4KB 与 PDF 11.4KB 双格式落盘
  - e2e 备注:工作项创建契约无 sprint_id 字段,关联须经 PUT /sprint(T-112 既有契约)
- [x] **T-213** 全文搜索:tantivy 索引管线(工作项/Wiki)+ 查询 API · [SRCH-01] · M ✅ 2026-10-10
  - tantivy 0.26 索引:title/body 用 NgramTokenizer(2..4,中文子串可搜);id/kind/project_id 用 raw 索引(delete_term 覆盖必需)
  - 管线:启动全量重建(PG 只读投影 proj_work_item + wiki_page)+ 订阅 workitem.*/wiki.* 事件增量 upsert(按 payload id 重读投影)
  - API:`GET /api/search?q=&kind=&limit=`(kind 可空;空查询 400;limit ≤ 50)
  - e2e:全量重建 31 篇;中文"崩溃"命中"登录崩溃"(BM25 4.0);kind=wiki 过滤 "Hello Wiki" 命中;新建工作项 3 秒内经事件可搜("量子加速导出管道")
  - tantivy 0.26 适配:NgramTokenizer 命名/as_str(CompactDocValue 经 Value::as_value→Leaf)/TopDocs.order_by_score()/writer 包 Mutex
- [x] **T-214** 前端:PR 页面(对话流/行内评论/diff 侧栏评论)+ 分支页 + Release 页 · L ✅ 2026-10-10
  - api-client 扩展:PR 列表/详情/diff/评论/评审/聚合/状态/评审人/合并/关闭重开 + tags/releases/分支保护(约 250 行契约)
  - RepoPage 四页签:文件/提交/PR/发布;RepoRoute 指向 pr-demo(数据丰富)
  - PullsView:列表(state 徽章/草稿标记)+ 新建表单 + 详情(评审聚合与 CI 检查徽章、通过/驳回按钮、三方式合并、关闭/重开、diff 着色视图点行锚定行内评论 📎、对话流卡片、评论输入)
  - ReleasesView:打标签 + 发布 Release(自动 Notes 展示)+ Release 卡片(附件)+ 分支保护规则表单(模式/最少通过/所有者评审)
  - e2e(真实浏览器):登录 → 代码页 → PR 列表 9 条 → PR#8 详情(评审 changes_requested + CI failure + diff + 对话流)→ 点 diff 行锚定 c.txt:7 发行内评论(对话流 1→2,📎 标记)→ 发布 tab(标签 2/Release 自动 Notes/保护表单);visual-judge 两页 verdict=pass
  - 顺手修既有 bug:fetchMyItems 未拆信封致工作台白屏(items.map is not a function)
- [x] **T-215** 前端:甘特图 + 需求池 + 报表页 + 导出交互 + Cmd+K 命令面板 · [PM-06,07, UX-02] · M ✅ 2026-10-10
  - api-client 扩展:fetchSearch / fetchItemsTree / listSprints / fetchReport(五类)/ fetchSprintReport / listProjectItems
  - 甘特图(workitem/GanttView):近 30 天时间轴刻度 + 状态色时间条(数据源 my-items,条 = 更新前 3 天 → 更新时点近似);需求池(BacklogView):items_tree 父子树(类型图标/编号/状态徽章,缩进层级)
  - 报表页(insights/ReportsView):消费五类报表 — 迭代报告(下拉选迭代,数字摘要)、缺陷趋势(红色条形 14 天)、吞吐(绿色周条)、累计流表格、成员负载(蓝色条形)
  - Cmd+K 命令面板(search/CommandPalette):全局快捷键 Cmd/Ctrl+K,导航命令过滤 + tantivy 全文搜索(250ms 防抖,Esc 关闭)
  - Shell 导航 += 甘特/需求池/报表;App 路由 /gantt /backlog /report
  - e2e(真实浏览器):甘特 30 行时间条渲染、需求池树(类型图标)、报表页五区块全数据(截图 visual-judge pass)、Cmd+K 开面板输"崩溃" → tantivy 命中(相关度 4.08)
  - 排障:loadErr early-return 跳过 useMemo 致 hooks 数变化整树卸载(移到 hooks 尾);items 列表接口无时间字段 → 换 my-items 数据源

**批次二出口自检**:全闭环走通;域账号登录;在 IM 里完成一次 PR 审批。

---

- [x] **T-216** 前端 UI 系统升级(用户反馈"界面难看,参考 gitcode.com")· [UX-01] · M ✅ 2026-10-10
  - 设计令牌精修:GitCode 蓝主色 #1F74EB、层次化表面/边框(border-light)、三级阴影、primary-soft 辅助底色
  - **全局组件样式**(apps/main/src/global.css):元素级默认(button/input/select/card/table)全页面继承 + .cp-* 类(cp-header/logo/nav-item/user-chip/avatar/kbd/card/table/badge/container)
  - Shell 重写:渐变 CP logo 徽标 + pill 导航高亮 + ⌘K 搜索提示 + 头像区;内容 1280px 容器;登录页渐变背景
  - 排障:白屏根因 = **fetchMe 同样未拆信封** → display_name undefined → Shell 头像 slice 崩 → React 19 整树卸载(与 fetchMyItems 同型 bug,一并修复)
  - **2026-10-11 重做**(用户:"现在的不好看,参考 gitcode"):用浏览器实测 gitcode.com/gitee.com 计算样式取真值(主色 #2C7EF8、控件圆角 4px、卡片 8px、顶栏 56px、边框 #DCE3E8),重写 tokens.css + global.css 组件库(按钮/卡片/表格/下划线标签页/徽章/空态/看板/diff/模态/统计卡),Shell 与 12 个页面全部改为类驱动、清除内联硬编码色;新增 ErrorBoundary + 修 adminListUsers 信封白屏;muted 对比度 2.78→4.52:1(AA)
  - e2e:工作台/报表页新视觉截图;visual-judge 两页 verdict=pass(遗留打磨:状态徽章配色、报表卡留白)提交触发流水线;应用一键部署并经域名健康上线)

### CI/CD 与制品

- [x] **T-301** workflow YAML 解析(Actions 兼容子集:jobs/steps/uses/run/矩阵)+ 校验器 + 超范围明示报错 · [CI-01] · L ✅ 2026-10-10
  - ci 域落地:workflow.rs 纯解析层(serde_yaml),Actions 兼容子集 — on(push.branches/manual)/jobs(runs-on 标签/steps/strategy.matrix)/steps(uses 白名单 4 个官方 action、run、with/env 字面量)
  - **超范围明示报错**:顶层 env/concurrency/schedule 等逐项提示 + 替代建议(job.needs → v1 独立执行、step.if/timeout、uses 未知 action 附可用列表);错误带字段路径(jobs.b.steps[0])
  - 矩阵:展开组合(N 维笛卡尔)+ `${{ matrix.dim }}` 占位渲染;用途说明(T-303 执行时逐组合)
  - 校验 API:`POST /api/ci/workflows/validate`(valid + workflow JSON + 矩阵组合数 / 422 + path+message)
  - 单测 6 项:合法解析、三类超范围明示、未知 action/缺字段/互斥、矩阵展开渲染、触发器变体、YAML 语法错;e2e 三场景全通
- [x] **T-302** Runner:bollard 容器调度 + runner 池/标签 + GitHub 官方 runner 二进制复用 · [CI-02] · L ✅ 2026-10-10
  - executor.rs:StepSpec/StepOutcome 原语(输出尾部 64KB 截断);**双后端** — Shell(本机 sh -c,cwd=workdir)与 Docker(bollard 0.21,每步一容器:镜像可配/workdir bind-mount /workspace/stdout+stderr 合并/结束删容器)
  - Docker 连接:DOCKER_HOST → /var/run/docker.sock → ~/.docker/run/docker.sock(macOS Desktop)逐候选探测(macOS e2e 实测走第三候选)
  - runner.rs:RunnerPool(注册覆盖/心跳/标签匹配 + 原子占用互斥/id 序稳定选择)+ **job 编排**(逐步 fail-fast,StepRecord 收集)
  - API:注册(外部执行器复用入口,同 id 覆盖)/列表/心跳;server 启动自动注册内置 `local-shell`(CODEPLAN_RUNNER_LABELS 可配)
  - 测试 6 项(shell echo/env/失败码/workdir、池标签匹配互斥释放、job fail-fast)+ **Docker 冒烟 2 项**(容器内读挂载文件,`-- --ignored` 实测);e2e:内置 runner 自动注册/外部 docker runner 注册/心跳/幽灵 404/列表
- [x] **T-303** 流水线执行引擎:调度状态机 + 实时日志回流(SSE)+ 构建缓存卷 · [CI-01,02,05] · L ✅ 2026-10-10
  - 数据模型(0022/0023):ci_pipeline(repo+name upsert,YAML+parsed)/ ci_run(queued/running/success/failure + trigger push/manual)/ ci_run_step(job+combo+seq 唯一约束 upsert)
  - Engine:单 worker 队列;状态机 queued→running→success/failure(worker Err 兜底 failure,不卡 running);worktree 检出(--git-dir 前置修复;checkout 失败即 failure);实际 SHA 回填(dispatch 只给分支也能上报);矩阵逐组合;**构建缓存卷**:宿主 cache_root/cache/{repo} 持久目录 + CACHE_DIR 环境变量(shell 复用/docker 挂载)
  - SSE:`GET /api/ci/runs/{id}/logs/stream`(broadcast channel,事件 queued/step_started/step_finished/run_finished)
  - 触发:push 自动(commit.pushed 订阅 → 分支匹配 push_branches → enqueue)+ 手动 dispatch;**闭环**:ci.run.finished 事件 → server 桥 → code-review set_status_by_system(新增,无 RBAC)→ rev_commit_status → PR 状态检查门禁
  - API:pipelines CRUD + dispatch + runs 列表 + run 详情(steps)+ SSE
  - e2e:手动 dispatch success(两步中文名/输出捕获)→ 事件落库 → rev_commit_status 回写;push "auto trigger" 自动触发 run success(sha 回填)+ status 闭环;SSE 事件流(queued/step_started/step_finished ×2/run_finished)
  - 排障:git worktree --git-dir 位置;ci_run_step ON CONFLICT 需 UNIQUE(sqlx checksum=SHA-384,库校验和修正)
- [x] **T-304** 制品库:Maven/npm/PyPI/容器镜像协议 + 产物上传下载 + 流水线产物打通 · [CI-03] · L ✅ 2026-10-10
  - 数据层(0024/0025):reg_artifact(repo/kind/name/version/filename 唯一 upsert,sha256;registry 级公共空间 repo_id=nil 无 FK)+ OpenDAL artifacts/ 对象存储
  - 四协议最小可用:**Maven layout**(GET/PUT group.path:artifact:version:file)/ **npm registry**(publish _attachments base64 → packument dist-tags.latest + tarball 下载)/ **PyPI**(twine multipart → PEP 503 索引页 hash 链接 + 文件)/ **OCI v2**(/v2/ ping、manifests put/get(ETag=digest)、blobs digest 寻址上传下载)
  - 通用 REST:/artifacts/{owner}/{key}(pipeline 产物列表)/ PUT 上传 / GET 下载
  - **流水线产物打通**:Engine run 结束扫描 workdir/dist/* 自动归档(version = sha 前 8 位,meta.run_id)
  - e2e:四协议 put→get 内容逐字节一致(maven jar/npm tgz+packument/pypi 索引页/OCI manifest+blob digest 校验)+ push 触发流水线后 dist/app.bin 自动归档(2a456467)
  - 排障:registry 级 repo_id=nil 与 FK 冲突(0025 移除);sqlx checksum 又一次(0022 勿再动)
- [x] **T-305** 安全扫描:Semgrep/gitleaks/trivy 容器执行编排 + 结果模型(官方插件)+ PR 门禁卡点 · [CI-04] · M ✅ 2026-10-12
  - 结果模型(迁移 0027 `ci_scan_finding`):repo/commit/tool/severity/rule/file/line/message;`PgScanStore` 按 (repo,sha,tool) 先删后插保证重扫幂等
  - 编排(`scan.rs`):`git archive` 把引用物化到**临时目录**(不污染裸仓库/工作树)→ 容器扫描 → 报告落挂载点内文件由宿主读回(避开执行器 stdout/stderr 混流)→ 解析 → 清理;三工具 JSON 差异收敛为**纯函数解析器**(6 项单测)
  - 执行器增强:`StepSpec` 增加 `entrypoint` 覆盖 —— gitleaks/semgrep/trivy 镜像自带 ENTRYPOINT,不清空则 `sh -c` 被当作子命令(实测报 `unknown command "sh"`)
  - 门禁:L3 之间不互相依赖,故发 `ci.scan.finished` 事件(含严重程度计数与 gated 标志),由**组合根**订阅并调用 review 服务写提交状态(critical/high 存在 → `failure`),与既有 `ci.run.finished` 桥同构
  - API:`POST/GET /api/repos/{owner}/{key}/scans`(触发 + 查询发现)
  - e2e(**真实容器**):建仓 → 推入私钥文件 → gitleaks 检出 1 项(rule=private-key,path 归一化为 `id_rsa`,severity=high)→ `gated:true` → 提交状态 `scan/gitleaks=failure`「已阻断合并」→ 状态查询接口 combined=failure;错误路径:不支持工具 400 / 仓库不存在 404 / 未认证 401
  - 顺带修 pre-receive 钩子**误导性报错**:回连平台失败(curl 000)时原样报 "push rejected by branch protection",实际与分支保护无关(本次 e2e 即被带偏),现区分「无法回连钩子端点」并提示检查 `CODEPLAN_PUBLIC_URL`
  - 偏差:扫描能力落在 ci crate 而非经插件扩展点接入 —— `ext` 的「CI step」扩展点当前仍是 trait 占位(见 ext/lib.rs),待其充实后再迁为插件;暂缺扫描结果的前端展示页

### 搜索与度量

- [x] **T-306** 代码搜索:tantivy n-gram 索引 + push 事件增量 + 查询 API 与前端 · [SRCH-02] · M ✅ 2026-10-12
  - 索引扩展:`DocKind::Code`(存储值 `code`)+ 新增 `code.rs`:文本扩展名白名单、体积上限 256KB、单仓文件数上限 2000、跳过 `node_modules/target/dist/vendor` 等目录(纯函数 `is_indexable`,4 项单测)
  - 读路径:`git ls-tree -r --name-only` + `git show <ref>:<path>` 直读裸仓库(CLI porcelain,不引 git2)
  - 全量:`rebuild_code` 遍历未归档仓库默认分支(27 个文件);增量:`spawn_code_index_worker` 订阅 `commit.pushed`,按 refs 行的 `new` 提交走 `diff-tree --name-only -r --root` 只索引**本次变更**文件
  - 查询:复用既有 `/api/search`,新增 `kind=code` 过滤(前端 Cmd+K 图标区分 wiki/code/workitem)
  - 排障两处:`git ls-tree` 的 `--name-only` 与 `-l` 不能同用(曾致每个仓库 SR-500);命中结果 kind 曾写死「wiki 否则 workitem」二选一,导致 code 命中被误报为 workitem → 改为按存储值 `DocKind::parse` 回读
  - e2e:推入含独特标识的文件 → 增量索引立即可搜到(`src/searchprobe.rs` 全文片段);kind 三态过滤正确(code=2 / wiki=0 / workitem=0);中文检索无回归;非法 kind 400
  - 说明:`repo_repository` 只做**只读投影**查询定位仓库(与 ci 域既有做法一致),不写他域数据
- [x] **T-307** DORA:事件聚合事实表 + 四指标计算 + 下钻查询 · [MS-01] · M ✅ 2026-10-12
  - 事实表(迁移 0028 `ins_dora_fact`):kind(deploy/failure/change)+ occurred_at + lead_time_seconds + subject/ref/detail;唯一索引 `(repo_id,kind,ref,occurred_at)` 保证事件重放幂等
  - **口径明确定义**(部署域未落地前的 v1,写入代码注释与文档,非猜测):`deploy` = 默认分支上成功的流水线运行;`failure` = 默认分支上失败的运行;`change` = 合入默认分支的 PR(lead_time = 合并时刻 − PR 创建时刻)。批次四发布域落地后 `deploy` 改由真实发布事件写入,**指标算法无需改动**
  - 采集(`dora_listen.rs`):订阅 `ci.run.finished`(按 `repo_repository.default_branch` 过滤,非默认分支的运行不算部署)与 `pr.merged`(仅 `merged_to_default`)
  - 四指标(`dora.rs`,9 项单测):部署频率(窗口次数/周)、前置时间(中位数与 p85,最近秩百分位)、变更失败率(失败/(部署+失败))、MTTR(每次失败到其后**首次**部署的间隔均值,末尾未恢复单独计数)
  - 下钻:`GET /api/repos/{owner}/{key}/dora/{metric}?days=` 按指标口径返回事实明细
  - e2e(**真实事件**):建两条流水线(成功/失败)→ 派发 成功→失败→成功 → 事实表 3 条;建 PR 并合并 → change 事实(lead_time=2s);四指标实测:部署频率 2 次/30 天 = 0.47 每周、失败率 1/3 = 33.33%、MTTR 平均 6 秒(与派发间隔一致)、前置时间 n=1 中位数 2s;下钻按口径过滤正确;未知指标 400 / 仓库不存在 404 / 未认证 401
  - 排障:首次派发全部 `无可用 runner` —— 内置 runner 的**标签**默认 `ubuntu-latest,shell`,`local-shell` 只是其名字(测试数据用错,非代码缺陷)
  - 说明:同 insights 既有定位,只对 `repo_repository`/`rev_pull_request` 做**只读投影**
- [x] **T-308** 效能报表(ECharts)+ Excel 导出 + 外部链接器 · [MS-02,03] · M ✅ 2026-10-12
  - 效能四指标(`efficiency.rs`,5 项单测):PR 周期(创建→合并)、评审响应(创建→**首次**评审)、交付周期(终态项 创建→更新)、缺陷逃逸率(迭代结束后才暴露的缺陷占比);口径写入注释便于复核
  - 统计助手 `stats.rs`:分位数与时长分布从 dora 上提(出现第二个真实调用者后),DORA 与效能报表共用
  - API:`GET /api/projects/{key}/reports/efficiency`、`GET /api/repos/{owner}/{key}/reports/pr-metrics`
  - **外部链接器**(迁移 0029 + `linker.rs`,5 项单测):模板占位符 `repo/sha/branch/item_number/pr_number/tag`,范围 global/repo/project 且具体范围优先;未知占位符在写入时即拒绝(400 并列出可用占位符),解析时保留原样便于发现配置错误
  - API:`GET /api/admin/external-links` / `POST`(平台管理员,同名覆盖)/ `DELETE`;`GET /api/repos/{owner}/{key}/external-links?sha=&branch=` 按上下文解析
  - 前端:**引入 ECharts** 重绘四张图(缺陷趋势折线、吞吐柱状、累计流堆叠面积、成员负载横向条),配色读 design-tokens;**懒加载**(`React.lazy` + `Suspense`)—— 图表块 537KB(gzip 183KB)仅在打开报表页时载入,主包维持 351KB(gzip 104KB);新增「交付与质量」卡片;仓库页头部渲染解析后的外部链接
  - e2e:项目效能(交付周期 n=2 中位 36s)+ PR 效能(pr_cycle n=1)+ 建 2 条全局链接 → 仓库页解析出 `var-repo=admin/scan-demo2&var-sha=…&var-branch=main` + 未知占位符 400 + 列表;浏览器实测 4 个 canvas 渲染、效能卡片就位、仓库页显示「↗ Grafana 构建看板 / ↗ SonarQube」;全站 9 路由无回归
  - 附注:Excel/PDF 导出由 T-212 交付,本任务未改动

### 部署引擎底座

- [x] **T-310** appcenter:应用模型 + 聚合视图 + 环境管理(部署目标登记) · [PAAS-01,02] · M ✅ 2026-10-12
  - crate 落地(此前仅 3 行占位):`domain`(应用/环境/部署目标/变量 + 聚合视图 + 仓储 trait)/`infra`(PG)/`service`(用例+权限)/`api`(路由+DTO+utoipa)
  - 迁移 0031 五表:`app_application` / `app_environment`(dev/test/staging/prod,生产标记)/`app_deploy_target`(主机+端口+标签)/`app_env_var`(敏感标记)/`app_repo_link`(应用↔仓库聚合)
  - 应用键规则(2~40 位小写字母/数字/连字符)与 **纯函数校验 + 环境类型往返**(2 项单测);`app.created` 事件 → identity 自动授予创建者 app admin(与 repo/project 同构)
  - **敏感变量双重脱敏**:写入响应与列表接口都不回显 `is_secret` 的值(仅返回 `••••••` 语义的 null)
  - 聚合视图 `GET /api/apps/{key}`:应用 + 环境(含目标数/变量数/生产标记)+ 关联仓库(名称经 `repo_repository` **只读投影**取,不写他域)
  - 端点 12 个:应用 列表/创建/聚合;环境 列表/创建/删除(级联目标与变量);目标 列表/登记/移除;变量 列表/写入/删除;仓库 关联/解除
  - 前端:新增 `@codeplan/feature-appcenter` 包 + 应用中心页(应用列表 / 环境卡片含计数与生产标记 / 目标登记 / 变量写入含敏感开关 / 仓库标签),接入导航、工作台侧栏与命令面板
  - e2e:建应用 → 非法键 400 / 重名 409 → 建 dev+prod(生产标记正确)→ 非法环境类型 400 → 登记目标(标签)→ 非法端口 400 → 写普通/敏感变量(**两处均脱敏**)→ 关联仓库 → 聚合视图计数与仓库名正确;浏览器实测面板渲染 + 界面新建应用/环境 + 非法键提示
  - 顺带:`ProjectService` 之后 `AppCenterService` 直接按 3 参数设计(仓储 + authz + events),不再走参数膨胀老路
- [x] **T-311** agent:Rust 单二进制(tonic gRPC 外连注册/token/心跳)+ 主机管理(DeployHost) · [PAAS-03] · L ✅ 2026-10-12
  - **契约**:`proto/codeplan/agent/v1/agent.proto`(gRPC:`Register`/`Heartbeat`/`Watch` 服务端流);方向约定写进 proto 注释 —— agent **主动外连**平台(主机侧零入站端口),故平台为 server、agent 为 client
  - **agent 二进制**(`crates/agent`,2 单测):tonic 0.14 客户端;注册(一次性 token→host_id)→ 心跳循环(间隔取平台建议值并**夹在 5~300 秒**)→ 命令流常驻;标签解析与间隔夹取为纯函数
  - **平台侧**(appcenter):迁移 0036 `app_deploy_host`(令牌**只存 SHA-256 哈希**,明文仅下发一次);`HostStatus` 由最后心跳时间**实时判定**(3× 间隔宽限,纯函数 4 单测);用例 登记/列表/注册/心跳;API 2 个
  - **gRPC 网关**在组合根(L4)实现并委托 appcenter —— L3 域之间不互相依赖,契约与装配都在 L4
  - 构建链:tonic 0.14 已把 prost 部分拆为 `tonic-prost`/`tonic-prost-build`,按新栈接入(protoc 34.1)
  - **排障**:① 命令流**立即结束、agent 每 5 秒重连** —— 我丢掉了 mpsc 发送端,流自然关闭 → 引入 `AgentHub` 持有发送端(T-313 从这里投递命令);② `send`/`online_count` 一度成为死代码被 `-D warnings` 拦下 → 移除尚未使用的 `send`(留到 T-313)、`online_count` 接入每小时运维日志,不留死代码
  - e2e(**真实 agent 二进制**):登记主机 → 下发令牌 → 跑 `./target/debug/codeplan-agent` → **注册成功**(host_id 返回、标签 `["linux","amd64","gpu"]` 回写)→ 心跳按 30 秒持续上报 → 平台侧 `node-4 registered=True status=online hostname=fushilu-2349.local ver=0.1.0` → **令牌一次性**:同令牌二次注册被拒「该注册令牌已被使用」;未注册主机保持 `pending`
- [x] **T-312** 服务规格:声明式版本化(镜像/端口/环境变量/卷/健康检查/副本数)+ 规格即代码 · [PAAS-03] · M ✅ 2026-10-12
  - 迁移 0032 两表:`app_service`(应用 × 环境唯一 + `current_version` 指针)+ `app_service_spec`(版本号唯一,规格**不可变**留痕,含原始声明文本 `source` 与提交人)
  - 规格模型(`ServiceSpec`):image / replicas / ports(container+protocol) / env / volumes(name+mount_path+size_mb) / healthcheck(path+port+interval+timeout);**`deny_unknown_fields`** —— 拼写错误不再被静默忽略
  - 校验为**纯函数**且一次返回**全部**问题(9 项单测):镜像须带 tag、副本 1~20、端口不重复且协议受限、变量名合法且不重复、卷挂载须绝对路径且不重复、健康检查须以 / 开头/端口已声明/超时小于间隔
  - **规格即代码**:`PUT .../spec` 接受 YAML 或 JSON 文本 → 解析 → 校验 → 追加新版本并置为当前;发布 `service.spec_applied` 事件
  - API:`GET/PUT /api/apps/{key}/environments/{env_id}/spec`、`GET .../spec/versions`(历史新→旧)
  - 前端:应用中心环境卡片内新增「服务规格」区块(当前版本徽章 + 镜像/副本/端口摘要 + 版本历史展开 + YAML 提交框)
  - e2e:初始 `data:null` → 提交 YAML(v1)→ 再提交(v2)→ 校验失败一次报出 2 个问题 → 未知字段拒绝并列出期望字段 → 历史 v2/v1 → 当前指向 v2 → 跨应用环境 400;浏览器实测版本徽章 v2 → 界面提交后 v3、摘要同步更新
  - 依赖:appcenter 新增 `serde_yaml`(与 ci 的 workflow 解析同源)
- [x] **T-313** 部署执行:期望状态调和 + agent 本地 bollard 拉镜像/起容器 + 健康检查 · [PAAS-04] · L ✅ 2026-10-13
  - **期望状态调和**(平台侧):规格(镜像/端口/卷/健康检查/副本数)+ 当前配置 → 生成容器计划;**`${KEY}` 引用从配置解析**,未定义即**拒绝下发**并列出缺失键(实测生效);副本数展开为 N 个稳定命名容器 `codeplan-{app}-{env}-{i}`(同名先移除再建 = 幂等收敛)
  - **配置注入不落明文**:部署计划在内存中携带解密后的配置与密钥,经 gRPC 直投 agent,不写库、不经 HTTP
  - **agent 本地执行**(`deploy.rs`/`executor.rs`,9 单测):bollard 拉镜像 → 移除旧容器 → 创建(端口映射/卷/环境变量/`unless-stopped` 重启策略)→ 启动 → **健康检查**(零依赖 HTTP 探活,按 interval 轮询至 timeout)→ 回执
  - **回执路由**:proto 新增 `ReportCommand`;平台侧 `app_command` 表把 command_id → 部署+主机,回执汇总为 `pending/success/failure`(纯函数 `aggregate_status`,5 单测)
  - API:`POST/GET /api/apps/{key}/environments/{env_id}/deployments`
  - **排障两处真缺陷**:① 规格协议是应用层 `http`/`grpc`,Docker 端口只认 `tcp`/`udp` → 直接透传导致 `docker start` 报 **unknown protocol**(agent 把真实原因回传,一眼定位)→ 加 `docker_protocol()` 归一化并单测;② 主机掉线后回执永不到达 → **部署永远挂 pending**(运维工具不能有悬空状态)→ 补 5 分钟**超时收敛**;顺带发现"在线判定"的 3× 间隔宽限会把**刚死掉的主机**选为部署目标 → 部署改按**心跳新鲜**(1 个间隔内)选目标(`is_fresh_for_deploy`,单测)
  - e2e(**真实部署**):规格 `nginx:alpine` + 健康检查 `/` → 触发部署 → agent 收命令 → 拉镜像起容器 → **容器 `0.0.0.0:80->80/tcp` 运行中,直连探活 HTTP 200(真实 nginx 页面)** → 回执 `OK 1/1 容器就绪` → **部署 `status=success`**;另有反向验证:规格引用未定义配置项被拒、`http` 协议错误真实暴露、悬空部署被超时判失败(容器不受影响)、离线主机被排除在目标之外
- [ ] **T-314** 部署可观测:日志/状态实时回流(WebSocket)+ 发布进度可视化数据 · [PAAS-04] · M
  - **已完成(代码就位、编译通过、agent 单测 11 项通过,但端到端未实测)**:
    - proto 新增 `PushEvents(stream AgentEvent)`;`AgentEvent{host_id, deployment_id, kind: status|log, message, at}`
    - agent 侧:`EventQueue`(持久队列 + 通知,**断线可重连不丢事件**,上限 2000)+ `EventSink` + bollard 日志跟随(`follow_logs`,跟随上限 30 秒,仅部署执行期内)
    - 平台侧:`DeployEventHub`(环形缓冲 500 条 + broadcast;新连上先补历史再收实时)+ WebSocket 端点 `GET /api/deployments/{id}/observations?token=`(浏览器无法给 WS 设头,故用查询参数鉴权)
  - **已实测通过**:起真实 agent 触发部署,WebSocket 收到 **22 条事件**(2 条状态「开始部署 1 个容器」/「容器 ... 健康检查通过」+ **20 行真实 nginx 启动日志**,如 `/docker-entrypoint.sh: Configuration complete`);鉴权实测:伪造令牌 401、有效令牌 101 升级;端点缺少 WS 头时 400(升级提取器先于鉴权,属预期)
  - ⚠️ **未解决**:回执到达平台耗时 **约 24 秒**(用 200ms 粒度轮询实测),部署状态被人为延迟。已排除的方向:把日志跟随(30 秒)移出主流程放后台**无效**。**未定位真因**,怀疑与镜像拉取(`create_image` 每次都会联系 registry 校对新 digest)或健康检查轮询间隔有关,接手后建议在 agent 的 `execute()` 各阶段打点定位
  - **待做**:① 查清并消除上述 24 秒延迟;② 前端「部署记录 + 实时日志面板」;③ 发布进度可视化数据(进度/批次概念属批次四灰度,此处需给出前端可消费的数据结构)
  - 注意:日志跟随目前只覆盖部署执行期;容器长期运行期的日志订阅需另行设计(按需订阅而非全量跟随)
- [x] **T-315** 域名绑定:自动分配/自定义域名 + 网关路由生成(Nginx 配置生成/APISIX Admin API 适配) · [PAAS-05] · M ✅ 2026-10-12
  - 迁移 0034 `app_domain`:环境级绑定(host 全局唯一)、来源 auto/custom、TLS 开关与证书引用(T-316 签发后回填)
  - `gateway.rs` **纯函数**渲染(9 项单测):主机名校验(RFC1123 宽松版,支持 `*.` 泛域名)、自动域名分配 `{app}-{env}.{suffix}`(非字母数字压连字符,后缀可经 `CODEPLAN_APP_DOMAIN_SUFFIX` 配)、Nginx 站点配置(upstream 聚合目标 + 80/443 + 跳转 + WebSocket 升级头)、APISIX 路由对象(Admin API body,含 roundrobin nodes 与 http_to_https 插件)
  - 用例:绑定(校验+全局去重)/自动分配(**幂等**)/列表/解绑/网关路由组装(域名 + 部署目标 + **规格端口**)/渲染/落地;发布 `domain.bound`
  - API 6 个:域名 列表/绑定/自动分配/解绑、网关 nginx 渲染 / apisix 渲染 / 落地
  - 落地策略:写 `CODEPLAN_GATEWAY_CONF_DIR` 下的 `{app}-{env}.conf`(部署引擎据此 reload),未配置则明确报错而非静默假装成功;APISIX 侧输出 Admin API 路径与 body 供运维/PaaS 落地
  - **排障(真缺陷)**:`tls_enabled=true` 但证书未签发时,生成的配置含 `listen 443 ssl` 却无 `ssl_certificate` —— **`nginx -t` 直接失败、reload 会让网关起不来**。单测只断言"包含哪些行",没人验证"配置合法"。修法:引入 `tls_active() = tls_enabled && cert_ref 非空`,证书未就绪即退化为明文 80;并补不变量测试(出现 `443 ssl` 必伴随 `ssl_certificate`)
  - e2e(**真实 nginx 校验**):自动分配(幂等)→ 自定义绑定 → 非法主机名(单段/下划线)400 → 重复 409 → 列表;网关 nginx 渲染(上游端口取规格 8080 而非 SSH 22)→ `nginx -t` **通过**;回填 cert_ref + 挂载自签证书后重渲染 → 出现 80 跳转与 443 ssl → `nginx -t` **再次通过**;落地文件落盘校验;浏览器实测域名面板(自动/自定义徽章、TLS 已签发/待签发)、网关预览、落地提示
- [x] **T-316** 证书插件 cert-acme:ACME 申请(Let's Encrypt/ZeroSSL,泛域名 DNS-01)+ 自动续期 + 部署钩子到网关/LB/主机 · [PAAS-07] · L ✅ 2026-10-12
  - **扩展点**:`ext::CertIssuer`(证书签发契约,含 trait 可用性单测)—— 平台只依赖契约,签发细节留在插件
  - **官方插件** `plugins/cert-acme`(5 项单测):ACME v2 复用社区 `instant-acme`(非自研);DNS-01 下发**可插拔**(`FileSolver` 文件 / `HttpSolver` 管理端点,适配 pebble-challtestsrv 与 DNS 网关);账号凭据持久化避免重复注册触发限流;`dns01_record_name` 正确处理泛域名与末尾点;从 PEM 解析 `not_after`(缺失会导致续期任务无从判断,故解析失败按错误处理)
  - **平台侧**:迁移 0035 `app_certificate`;用例 签发(收集启用 TLS 的域名 → 签发 → **证书落盘为部署钩子** → 记录 → **回填 `app_domain.cert_ref`** → 发 `cert.issued`)/ 列表 / 续期扫描;API 2 个;续期 worker 每小时扫描一次(14 天窗口)
  - 与 T-315 的衔接:回填 `cert_ref` 后网关渲染的 TLS 分支(`tls_active()`)自动生效,无需人工改配置
  - **排障三处真缺陷**:① 依赖树同时含 aws-lc-rs 与 ring 时 rustls 无法自动选定 provider → **首次 TLS 握手 panic**(签发请求 0.5 秒空响应)→ 启动时显式 `install_crypto_provider()`;② 插件未解析证书有效期 → `not_after` 为 NULL,而续期查询把 NULL 当到期 → **每小时都会重签**、白烧 CA 限流 → 解析有效期 + 续期查询收紧为 `not_after IS NOT NULL`;③ 组合根引用新插件未登记 → **lint-deps 拦截**(门禁按预期生效)
  - e2e(**真实 ACME**):本地起 **pebble + pebble-challtestsrv** 作为 ACME 测试 CA → 申请证书 4.7 秒完成 → 证书为真实 X.509(`issuer=CN=Pebble Intermediate CA`,SAN 含两域名,有效期 6 天)→ 落盘 `.crt`(1312B)+ `.key`(241B,权限 600)→ `cert_ref` 回填两个域名 → 网关重渲染出现 `listen 443 ssl` + 证书路径 → **`nginx -t` 通过**;续期判定实测(14 天窗口命中、3 天紧急窗口未命中);浏览器实测证书面板显示「web-portal-dev · cert-acme · 至 2026-10-18」
- [x] **T-317** 配置与密钥:AppConfig 版本化 + 敏感项加密存储 + 注入不落明文 · [PAAS-06] · M ✅ 2026-10-12
  - 迁移 0033 两表:`app_config`(应用 × 环境唯一 + `current_version`)+ `app_config_version`(版本唯一,配置项 JSONB,含留痕)
  - **加密**(`crypto.rs`,6 项单测):AES-256-GCM 认证加密,存储 `base64(nonce‖密文‖tag)`,每条独立随机 nonce(同明文两次密文不同);主密钥取 `CODEPLAN_CONFIG_KEY`(base64 32 字节),缺失时退化为固定开发密钥并**启动告警**。测试覆盖:往返、同明文异密文、换密钥必失败、**篡改末字节被 GCM 检出**、非法输入、开发密钥跨重启可解
  - 版本化:提交 → 解析 → 加密敏感项 → 追加新版本并置当前;发布 `config.applied`
  - **注入不落明文**:新增 `resolved_config_items()` 供部署链路在内存中取明文(不经 HTTP 暴露);HTTP 仅提供**单键解密** `POST .../config/reveal`,需写权限且发 `config.secret_revealed` 审计事件(**不记录值**)
  - API:`GET/PUT /api/apps/{key}/environments/{env_id}/config`、`GET .../config/versions`、`POST .../config/reveal`
  - 前端:应用中心环境卡片内新增「配置与密钥」区块(版本徽章 / 键值列表(敏感项 `••••••`)/ 单键解密按钮 / 版本数 / YAML 提交框)
  - **排障两处**(均为真实缺陷):① 留痕字段 `source` 原样保存声明文本 → 敏感值**明文落库**,与"注入不落明文"直接冲突 → 新增纯函数 `redact_source` 在落库前把敏感值替换为占位符;② `parse_config` 只接受字符串值,导致 `PORT: 8080` 这类**最普通的配置被判非法** → 标量(字符串/数字/布尔)一律强制转字符串,对象形态的 `value` 同样放宽
  - e2e(**密文落库实测**):提交含敏感项配置 → 响应中敏感项 `value=null`、DB 中该键为密文(肉眼可见 base64 密文)→ **全表 `items::text || source` 搜索明文 = 0 行**;单键解密返回明文且按钮转「已解密」;版本 v1→v4 递增;不存在的键 404;非映射配置 400;审计事件 `config.applied`/`config.secret_revealed` 均落 outbox;界面提交的敏感值同样 0 行明文
- [x] **T-318** 工时:登记/预估/统计(PM-09) · S ✅ 2026-10-12
  - 模型(迁移 0030 `proj_worklog`):work_item/author/minutes/note/spent_on(缺省今天);索引 (item, spent_on DESC)
  - 域:`WorklogId`/`Worklog`/`NewWorklog`/`WorklogRepository` + **纯函数 `summarize_worklogs`**(累计/按人汇总,2 项单测)
  - 用例:`log_work`(单条 1 分钟 ~ 24 小时校验 + `workitem.worklogged` 事件)、`list_worklogs`、`worklog_summary`(累计/预估/按人)、`delete_worklog`(登记者本人或项目写权限)
  - 预估:取自工作项 `custom_fields.estimate_minutes`(经既有 `PATCH /api/workitems/{id}` 设置),汇总时与累计一起返回并算完成度
  - API:`POST/GET /api/workitems/{id}/worklogs`、`GET .../worklogs/summary`、`DELETE .../worklogs/{worklog_id}`
  - 前端:工作项详情页新增「工时」面板(累计徽章 + 预估进度条 + 明细 + 登记表单)
  - 重构:`ProjectService::new` 参数达 8 个触发 clippy 上限 → 收拢为 `ProjectStores` 依赖结构体(3 参数)
  - e2e:登记(带日期/缺省今天)→ 非法用量 0/-5/2000 全部 400 → 汇总 135 分钟/2 条/按人 → 明细;浏览器实测面板显示「累计 2h15m」+ 界面登记后徽章变「2h45m」、明细 3 条

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
| 2026-10-10 | T-123/T-124 | 平台用户管理(列表/禁用启用,防自禁)+ /api/dashboard/* 聚合 + metrics 摘要 + deploy/backup.sh 实测;e2e 7 项全过 |
| 2026-10-10 | T-120 | git-service 域落地:仓库生命周期 + 裸仓库 git init + repo.created 自动授权;e2e 6 项全过;模板/导入/镜像随 T-121 后补 |
| 2026-10-10 | T-121 | smart HTTP 代理 + Basic 鉴权 + post-receive 钩子事件管道;真实 git clone/push 往返 e2e 全过(含无凭据 401) |
| 2026-10-10 | T-122 | GitBrowse adapter(CLI porcelain)+ 6 个浏览端点;e2e 7 项全过;blame 直接用 porcelain 无需朴素算法 |
| 2026-10-10 | T-201 | russh 0.64 SSH 服务(2322 端口)+ 公钥指纹认证 + upload/receive-pack 全双工管道;真实 git clone/push 走 ssh:// 全通;排障:诊断数据污染协议流掩盖真问题,根因 = 缺 exit-status/通道关闭收尾 |
| 2026-10-10 | T-202 | storage crate(opendal 0.59 fs)+ LFS basic transfer(batch/upload/download/verify,短时 HMAC 令牌双认证)+ SSH git-lfs-authenticate;真实 git-lfs 5MB 对象 HTTP/SSH 双路径 e2e 全通 |
| 2026-10-10 | T-203 | kernel git 契约 + git-service adapter + code-review 域四层;PR 全生命周期 API;merge/squash/rebase 三方式真实仓库 e2e 全通(排障:commit_env 未管道化 stdout 导致 update-ref 拿空 SHA)|
| 2026-10-10 | T-204 | 评审流:行内评论锚定校验 + 通过/驳回(自评拒绝/最新结论聚合)+ 状态检查聚合(none/pending/failure/success);双用户 e2e 全通,事件三类落库 |
| 2026-10-10 | T-205 | 分支保护引擎:合并门禁(评审聚合+状态聚合)+ pre-receive 推送门禁(直推/删除/force 拦截);e2e 全通;修复钩子 GIT_DIR 相对路径潜伏 bug |
| 2026-10-10 | T-206 | CODEOWNERS:解析器 + PR 创建自动指派 + require_code_owner 缺员门禁;kernel 补 file_content/by_username 契约;e2e 全通 |
| 2026-10-10 | T-207 | 提交关联:push 事件带提交消息 → #KEY-N 建链 → 反向展示;pr.merged 合入默认分支按 BFS 路径自动流转(bug→resolved);系统流转豁免缺陷门禁 |
| 2026-10-10 | T-208 | Tag/Release:kernel tag 契约 + 发布(upsert)+ 拓扑探测上一标签自动生成 Notes + OpenDAL 附件;e2e 全通 |
| 2026-10-10 | T-209 | Webhook 出站:NATS 全事件订阅 → HMAC 签名投递(3 次重试)→ 日志;真实接收器验签 e2e 全通 |
| 2026-10-10 | T-210 | 访问凭证:PAT(哈希存储/scope 门禁)+ GPG(sequoia)/SSH(ssh-keygen -Y)提交签名验证;e2e 双签名 verified + signer 归属;排障:split_gpgsig 偏移/consumed/cat-file trim 三处细节 |
| 2026-10-10 | T-211 | IM 卡片:统一卡片 + 企微/钉钉/飞书渲染与回调解析;回调端点 → IM 内审批 PR/流转工作项(身份映射 + 领域规则守卫);e2e 全通 |
| 2026-10-10 | T-212 | 报表引擎:insights 域(只读投影)+ 五类报表计算 + Excel/PDF 导出;e2e 双格式全通 |
| 2026-10-10 | T-213 | 全文搜索:tantivy(NGram 中文子串)+ 全量重建 + 事件增量;查询 API;e2e 中文/kind/增量全通 |
| 2026-10-10 | T-214 | 前端 PR/发布/分支保护三页面 + api-client 扩展;真实浏览器 e2e(行内评论锚定/评审/合并按钮)+ visual-judge pass;修 fetchMyItems 信封白屏 |
| 2026-10-10 | T-215 | 前端甘特/需求池/报表页 + Cmd+K;真实浏览器 e2e 四件套全通 + visual-judge pass;批次二完成(15/15)|
| 2026-10-10 | T-301 | CI workflow YAML 解析器(Actions 兼容子集 + 超范围明示报错 + 矩阵展开)+ 校验 API;单测 6 项 + e2e 三场景全通;批次三起步 |
| 2026-10-10 | T-302 | Runner 池/标签调度 + Shell/Docker 双执行后端(bollard 0.21 builder API)+ 注册/心跳 API;Docker 冒烟实测(挂载+输出);100 测试全过 |
| 2026-10-10 | T-303 | 流水线执行引擎:状态机 + worktree 检出 + 矩阵 + SSE + 缓存卷;push 触发→执行→commit status 闭环 e2e 全通(CI 门禁数据打通 T-204/T-205)|
| 2026-10-10 | T-304 | 制品库:Maven/npm/PyPI/OCI v2 四协议 + 通用 REST;流水线 dist/ 自动归档打通;四协议 e2e 全通 |
| 2026-10-10 | T-216 | 前端 UI 升级(GitCode 质感):令牌+全局组件样式+Shell 重写;修 fetchMe 信封白屏;visual-judge pass |
| 2026-10-11 | T-216 | UI 重做(用户反馈"现在的不好看,参考 gitcode"):实测 gitcode/gitee 计算样式→重建令牌(主色 #2C7EF8、控件 4px/卡片 8px 圆角、56px 顶栏)+ 全量 .cp-* 组件库;12 个页面全部重写去内联硬编码色;修 adminListUsers 信封白屏 + 新增 ErrorBoundary;muted 文字对比度提到 AA(4.52:1);全站 8 路由零横向溢出 |
| 2026-10-11 | T-216 | 按 ai-website-cloner 方法(Map→Observe→Build→Compare)克隆 gitcode 仓库页:浏览器实测 DevUI 令牌(主色 #165dff、实心按钮 #181818、正文 #252b3a、分隔线 #e3e3ee、内容 1210px、侧栏 400px);令牌与组件库对齐后 RepoPage 重构为「仓库头 53px + 标签栏 48px/间距 32px + 分支工具栏 32px + 双栏 769+400」;对照源站校验误差 <1%。brief 落 docs/design/gitcode-brief.md |
| 2026-10-11 | T-216 | 按 ai-website-cloner 方法(Map→Observe→Build→Compare)逐区块克隆 gitcode 首页:实测左频道栏 240px/#f9f9fb、hero 110px、胶囊行 53px、卡片 485×151 间距 16px、排名徽章/操作按钮/描述 2 行截断/页脚全部取值;WorkbenchPage 重构为发现页布局(真实项目与工作项数据);对照源站逐项一致。brief 见 docs/design/gitcode-brief.md |
| 2026-10-11 | T-216 | **配色全量克隆**:抓取 gitcode 全部 810 个 CSS 变量,取 DevUI 语义色板逐项映射(brand #165dff / primary #000 / link #2951e0 / text #252b3a / text-weak #3b3e55 / placeholder #9c9db4 / line #e3e3ee / dividing-line #f1f1f8 / global-bg #f9f9fb / area #f1f1f8 / success #088f4e / warning #ed5f22 / danger #da203e / info #5e7ce0),替换此前的近似色;卡片/胶囊/徽章等字面值(描述 #606060、卡片边框 #f3f4f6、排名 #ffe0d4+#b1472d、胶囊激活 #0f0f0f+#f1f1f1)全部对齐。26 项令牌 + 16 项元素颜色实测逐项一致;全站 8 路由无回归。注:#9c9db4 对比度 2.53:1 低于 AA(gitcode 原样),已在 brief 标注一键改回方案 |
| 2026-10-11 | T-134 | 修 `git ls-tree -l` 解析字段错位(把 mode 当 kind、type 当 oid)→ 目录恒显示为文件、size 恒空、oid 恒为 "blob";抽出 `parse_ls_tree_line` + 4 项回归测试。**已编译验证**:`cargo test -p codeplan-git-service` 19 项全绿、`cargo clippy -p codeplan-git-service -- -D warnings` 通过 |
| 2026-10-12 | T-132 | 工作项详情与列表(批次一收尾):评论域(迁移 0026 + 仓储/用例/事件)+ 详情·流转·评论 5 个端点(`transitions` 由服务端状态机裁定);前端 `WorkItemDetail`/`WorkItemListView` + 路由 `#/items`、`#/item/{id}` + 看板/需求池入口;修 api-client `ProjectItem` 契约错位(`item_number`→`number`、补时间字段)。e2e:后端 8 项 + 浏览器全流程(筛选→详情→评论→流转→刷新持久化→删除)全通;门禁 clippy/test(103 项)/lint-deps(22 crate)/前端 build+lint 全绿 |
| 2026-10-12 | T-135 | 管理页三标签 + 组织架构(ORG-01):`OrgRepository` 原已实现却从未接线(无 API/组合根、团队表空)→ 扩展组织+团队+成员仓储并接入 `AuthState`;新增 7 个管理员端点(组织/团队/成员 CRUD + 成员按登录名添加);前端 `AdminPage`(用户/组织/审计)、`AdminOrgsPage`(三级维护)、`AdminAuditPage`(动作着色 + 筛选 + 分页档位)。e2e:重名 409/不存在用户 404/非管理员 7 端点全 403/未认证 401 + 浏览器三标签实测;门禁 clippy/104 测试/lint-deps/前端全绿。**批次一 25/25 收尾** |
| 2026-10-12 | T-305 | 安全扫描(Semgrep/gitleaks/trivy):迁移 0027 发现表 + 纯函数解析器(6 单测)+ 临时目录物化 + 容器编排 + `entrypoint` 覆盖(gitleaks 镜像自带 ENTRYPOINT);门禁走 `ci.scan.finished` 事件由组合根回写提交状态。**真实容器 e2e**:检出私钥 → gated → `scan/gitleaks=failure`「已阻断合并」。顺带修 pre-receive 钩子把「回连失败」误报为「分支保护拒绝」的误导性错误。门禁 clippy/110 测试/lint-deps/前端全绿 |
| 2026-10-12 | T-306 | 代码搜索(SRCH-02):`DocKind::Code` + 文本白名单/体积与数量上限(4 单测)+ `git ls-tree/show` 直读裸仓库;全量重建(27 文件)+ `commit.pushed` 增量只索引变更文件;`/api/search?kind=code` 过滤 + 前端图标区分。排障:`ls-tree --name-only` 与 `-l` 冲突致 SR-500、命中 kind 写死二选一误报 code 为 workitem。e2e:推入独特标识 → 立即可搜;kind 三态正确;中文无回归。门禁 clippy/114 测试/lint-deps/前端全绿 |
| 2026-10-12 | T-307 | DORA 四指标(MS-01):迁移 0028 事实表(唯一索引保证重放幂等)+ 订阅 `ci.run.finished`(按默认分支过滤)/`pr.merged`;指标纯函数 9 项单测(部署频率/前置时间中位数与 p85/变更失败率/MTTR)+ 按指标口径下钻。**真实事件 e2e**:派发 成功→失败→成功 + 合并 PR → 四指标数值逐项核对(0.47 次每周 / 33.33% / MTTR 6s / 前置 2s)。排障:内置 runner 标签是 `ubuntu-latest,shell` 而非其名 `local-shell`。门禁 clippy/123 测试/lint-deps/前端全绿 |
| 2026-10-12 | T-308 | 效能报表 + 外部链接器(MS-02,03):效能四指标(PR 周期/评审响应/交付周期/缺陷逃逸,5 单测)+ 统计助手从 dora 上提共用;迁移 0029 外部链接器(模板占位符 6 种/三级范围/未知占位符写入即拒,5 单测)+ 5 个端点。前端**引入 ECharts** 重绘四图并**懒加载**(主包 351KB 不变,图表块 537KB 按需);仓库页渲染外部链接。e2e:效能端点数值 + 模板解析(含 sha/branch)+ 浏览器 4 canvas + 全站 9 路由无回归。门禁 clippy/135 测试/lint-deps/前端全绿 |
| 2026-10-12 | T-318 | 工时(PM-09):迁移 0030 + 纯函数汇总(2 单测)+ 登记/明细/汇总/删除四用例(含 `workitem.worklogged` 事件与权限判定)+ 4 端点;详情页「工时」面板(累计徽章/预估进度/明细/登记表单)。重构 `ProjectService::new` 参数超限 → `ProjectStores`。e2e:非法用量 400、汇总数值、界面登记徽章实时更新。门禁 clippy/137 测试/lint-deps/前端全绿 |
| 2026-10-12 | T-310 | 应用中心(PAAS-01,02):appcenter crate 从 3 行占位落地为四层(domain/infra/service/api);迁移 0031 五表(应用/环境/部署目标/变量/仓库聚合)+ 12 端点 + 聚合视图;敏感变量双端脱敏;`app.created` 自动授予创建者 app admin。前端新增 `feature-appcenter` 包与应用中心页并接入导航/侧栏/命令面板。e2e:键校验 400/重名 409/端口 400/生产标记/脱敏/聚合计数 + 浏览器界面写入。门禁 clippy/139 测试/lint-deps(22 crate)/前端(17 包)全绿 |
| 2026-10-12 | T-312 | 服务规格(PAAS-03):迁移 0032 两表(服务 + 不可变版本,含原始声明留痕);`ServiceSpec` 声明式模型 + `deny_unknown_fields` + 纯函数校验一次报全部问题(9 单测);规格即代码(YAML/JSON 文本 → 解析校验 → 追加版本)+ `service.spec_applied` 事件;3 端点;应用中心内规格区块(版本徽章/摘要/历史/提交框)。e2e:版本递增 v1→v2→v3、校验与未知字段拒绝、跨应用防护、界面提交。门禁 clippy/148 测试/lint-deps/前端(17 包)全绿 |
| 2026-10-12 | T-317 | 配置与密钥(PAAS-06):迁移 0033 两表(配置集 + 不可变版本);AES-256-GCM 认证加密(6 单测含篡改检出)+ 主密钥经 `CODEPLAN_CONFIG_KEY`;`resolved_config_items` 仅供部署链路内存取用,HTTP 只给单键解密并留审计;3 端点 + 应用中心配置区块。**排障两处真缺陷**:留痕字段明文泄漏敏感值(加 `redact_source`)、`PORT: 8080` 被判非法(标量强制转字符串)。e2e:全表明文搜索 0 行、解密/版本/错误路径全通。门禁 clippy/164 测试/lint-deps/前端全绿 |
| 2026-10-12 | T-315 | 域名绑定与网关路由(PAAS-05):迁移 0034;`gateway.rs` 纯函数渲染(主机名校验/自动域名/Nginx 站点/APISIX 路由,9 单测);6 端点 + 落地到网关配置目录;应用中心域名面板。**排障真缺陷**:TLS 声明开启但证书未签发时生成非法 nginx 配置(`listen 443 ssl` 无证书)→ `nginx -t` 失败会拖垮网关 → 引入 `tls_active()` 退化 + 不变量测试。e2e:**真实 nginx 校验两条分支均通过**。门禁 clippy/173 测试/lint-deps/前端全绿 |
| 2026-10-12 | T-316 | 证书插件 cert-acme(PAAS-07):`ext::CertIssuer` 扩展点 + `plugins/cert-acme`(复用 instant-acme,5 单测,DNS-01 可插拔)+ 迁移 0035 + 签发/列表/续期用例 + 部署钩子落盘与 `cert_ref` 回填 + 续期 worker。**排障三处真缺陷**:rustls provider 未选定致握手 panic、`not_after` 为空致每小时重签、插件未登记被 lint-deps 拦截。e2e:**pebble 真实 ACME 签发** → 真 X.509 → 落盘 → 回填 → 网关 443 → `nginx -t` 通过。门禁 clippy/179 测试/lint-deps(23 crate)/前端全绿 |
| 2026-10-12 | T-311 | agent + 主机管理(PAAS-03):proto 三方法(Register/Heartbeat/Watch 流)+ `crates/agent` 二进制(tonic 0.14 新栈 tonic-prost)+ 迁移 0036(令牌只存哈希)+ 在线状态实时判定(4 单测)+ L4 gRPC 网关委托 appcenter。**排障**:命令流因丢弃发送端而秒断致 agent 反复重连 → 引入 `AgentHub` 持发送端(T-313 投递口);死代码被 `-D warnings` 拦下 → 移除未用的 `send`。e2e:**真实 agent 二进制**注册+心跳+令牌一次性全通。门禁 clippy/185 测试/lint-deps(23 crate)/前端全绿 |
| 2026-10-13 | T-313 | 部署执行(PAAS-04):期望状态调和(规格+配置→容器计划,`${KEY}` 注入校验)+ agent bollard 拉镜像/起容器/健康检查 + proto `ReportCommand` 回执路由 + 部署记录与状态汇总(纯函数 5 单测)+ 5 分钟超时收敛。**排障**:应用层协议透传给 Docker 致 `unknown protocol`、主机掉线致部署永挂 pending、在线宽限把死主机选为目标。e2e:**真实部署 nginx:alpine → 容器运行 → 探活 HTTP 200 → status=success**。门禁 clippy/201 测试/lint-deps(23 crate)/前端全绿 |
