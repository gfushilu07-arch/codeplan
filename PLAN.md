# CodePlan — 企业内部一体化研发平台 · 项目规划(V1)

> 面向企业内部的项目管理 + 代码托管 + 发布交付一体化平台。项目管理对标 Jira/PingCode 的中重型研发闭环,代码管理对标 GitHub 核心能力,发布交付域内置**自研轻量部署引擎**(agent + Docker,无 K8s)、灰度发布与功能开关,让用户部署应用像"建应用 → 选环境 → 一键部署"一样简单——即**内部 PaaS + 研发管理闭环**。Rust 技术栈,最大化复用开源社区组件,私有化部署;官方能力以插件形态实现,插件框架为后续开放生态预留。
>
> **V1 一次性交付本文件所列全部功能(共 71 项)**;内部按四个构建批次推进(见第 5 节),批次四完成即 V1.0 发布。
> 规划日期:2026-10-09

---

## 1. 定位与设计原则

- **部署对象**:企业内部(单租户,内网/私有云),用户规模设计上限 ≤5000 人、仓库 ≤5 万、单仓库 ≤5GB(LFS 除外)。
- **技术栈**:Rust 全栈(后端),前端 React + TypeScript。
- **部署约束**:**不使用 Kubernetes**(明确过重)。平台自身交付形态为 Docker Compose(单机或少量节点 + Nginx 外部负载均衡横向扩展);**用户应用的部署由平台自研轻量部署引擎接管**(装了 Docker 的任意主机均可作为部署目标),与平台自身的部署形态无关。
- **工程原则**:
  1. 业务逻辑自研,基础件全部组装开源组件(只用活跃维护的头部项目、优先 Rust 原生、不引 C FFI);
  2. 模块化单体,模块间只经"内部 API trait + 领域事件"通信,不直连他模块数据表;
  3. Git 写路径信任 git 官方二进制(gitoxide 的 push 在纯 Rust 中未完成,读路径用 gix);
  4. 凡属"研发流程内"的能力(发布、开关、门禁)自研闭环;凡属"运行时生态"的能力(实验分析等)集成开源,平台做入口与联动。

### 非目标(明确不做)

- K8s/Helm 交付、K8s 原生编排;
- 部署引擎的过度通用化:只管理"装了 Docker 的主机上的长驻服务",不做虚拟机/裸金属(无 Docker)目标、不做定时任务编排(归 CI 域)、不做多云、V1 无自动弹性;
- SaaS 多租户、公有云社区功能(star/fork 网络、探索页、开源协作治理);
- 完整 A/B 实验平台(分桶埋点与指标分析)——集成 GrowthBook,不自研;
- 第三方动态插件与插件市场(WASM 沙箱、运行时安装)为 V2;V1 插件均为官方内置插件(随平台编译发行)。

---

## 2. 功能列表(V1 全集,共 71 项)

> 编号规则 `域-序号`,讨论与变更直接引用 ID;「批次」对应第 5 节构建批次;🔌 = 以官方内置插件形态实现。

### 2.1 组织与权限(ORG,7 项)

| ID | 功能 | 说明 | 批次 |
|---|---|---|---|
| ORG-01 | 组织架构 | 部门/团队/成员;LDAP/AD 定时同步与手动维护 | 1 |
| ORG-02 | 认证 | 本地账号(argon2)、OIDC 单点登录、LDAP 认证、TOTP 两步验证 | 1 |
| ORG-03 | 授权模型 | 系统角色 + 项目级角色 + 自定义角色(RBAC);仓库级细粒度 ACL | 1 |
| ORG-04 | 访问凭证 | SSH Key、GPG/SSH 提交签名验证(sequoia)、个人访问令牌(细粒度 scope) | 2 |
| ORG-05 | 审计日志 | 登录、权限变更、仓库/工作项操作全量审计 | 1 |
| ORG-06 | 审计合规报表 | 模板化合规报表(权限/仓库/导出记录),PDF/Excel 导出 | 4 |
| ORG-07 | 个人中心 | 偏好、通知订阅、我的工作项、SSH/令牌管理 | 1 |

### 2.2 项目管理(PM,13 项)

| ID | 功能 | 说明 | 批次 |
|---|---|---|---|
| PM-01 | 工作项体系 | 史诗→需求→任务/缺陷/测试用例;类型可扩展;自定义字段(JSONB) | 1 |
| PM-02 | 工作流引擎 | 每类型独立状态机:状态/流转/流转权限/必填校验可配置 | 1 |
| PM-03 | 迭代(Sprint) | 规划、容量、燃尽图、速率统计 | 1 |
| PM-04 | 里程碑 | 版本规划、关联工作项进度汇总 | 1 |
| PM-05 | 多视图 | 看板(拖拽/自定义列/WIP 限制)、列表(筛选/批量操作)、树形(需求分解) | 1 |
| PM-06 | 甘特图 | 时间线排期与进度 | 2 |
| PM-07 | 需求池 | 需求收集、评审、优先级排序 | 2 |
| PM-08 | 缺陷跟踪 | 严重程度/优先级、复现步骤、生命周期与回归 | 1 |
| PM-09 | 工时 | 预估/登记/统计报表 | 3 |
| PM-10 | 测试管理 | 用例库、测试计划、执行关联缺陷、测试报告 | 4 |
| PM-11 | Wiki | 文档树、Markdown(comrak)、附件、页面级权限继承 | 1 |
| PM-12 | 报表与导出 | 迭代报告/成员负载/缺陷趋势/需求吞吐/累计流图;Excel/PDF 导出 | 2 |
| PM-13 | 工作项通知 | 站内信、邮件、IM webhook;按事件订阅 | 1 |

### 2.3 代码管理(GIT,13 项)

| ID | 功能 | 说明 | 批次 |
|---|---|---|---|
| GIT-01 | 仓库管理 | 创建/导入/镜像、内网公开/私有、模板仓库、转移与归档 | 1 |
| GIT-02 | Git HTTP 协议 | smart HTTP(clone/push),git http-backend 代理 | 1 |
| GIT-03 | Git SSH 协议 | russh 或系统 sshd forced-command | 2 |
| GIT-04 | 代码浏览 | 文件树、README 渲染、提交历史、diff、blame(tree-sitter 高亮) | 1 |
| GIT-05 | 分支管理与保护 | 分支列表/删除、默认分支;保护规则:最少审批数、状态检查门禁、禁 force push | 2 |
| GIT-06 | CODEOWNERS | 按目录规则自动指派评审人,缺员阻塞可配置 | 2 |
| GIT-07 | Tag / Release | 版本发布、附件、从关联 PR/工作项自动生成 Release Notes | 2 |
| GIT-08 | LFS | 自实现 LFS API(batch 接口 + OpenDAL 转存) | 2 |
| GIT-09 | Pull Request | 创建/Draft、行内评论、评审通过/驳回、状态检查、merge/squash/rebase | 2 |
| GIT-10 | 提交关联 | commit `#ID` 关联工作项;PR 合并自动流转状态(可配规则);工作项反向展示分支/PR | 2 |
| GIT-11 | Webhook | 出站 webhook,事件订阅 + HMAC 签名 | 2 |
| GIT-12 | 代码片段 | 内部 Gist:私有/团队/全组织 | 4 |
| GIT-13 | 讨论区与仓库 Issue | 仓库级 Discussions;轻量 Issue 一键转项目工作项 | 4 |

### 2.4 CI/CD 与制品(CI,5 项)

| ID | 功能 | 说明 | 批次 |
|---|---|---|---|
| CI-01 | 流水线 | workflow YAML 兼容 GitHub Actions 核心语法子集,超出范围明示报错 | 3 |
| CI-02 | Runner | Docker 容器化(bollard)、标签分组;复用 GitHub 官方 runner 二进制 | 3 |
| CI-03 | 制品库 | Maven/npm/PyPI/容器镜像(OpenDAL + PG 元数据),与流水线产物打通 | 3 |
| CI-04 | 安全扫描门禁 🔌 | Semgrep/gitleaks/trivy 结果进 PR 状态检查,支持卡点 | 3 |
| CI-05 | 构建缓存 | Docker volume,按流水线/分支 | 3 |

### 2.5 发布与应用运行(PAAS,16 项)

| ID | 功能 | 说明 | 批次 |
|---|---|---|---|
| PAAS-01 | 应用中心 | "应用"一等实体:聚合仓库/流水线/环境/发布记录/外部监控链接 | 3 |
| PAAS-02 | 环境管理 | 开发/测试/预发/生产与部署目标(主机群)登记、变量注入 | 3 |
| PAAS-03 | 部署引擎 | agent(token 注册、gRPC 主动外连、零入站端口);服务规格声明式、版本化、可评审 | 3 |
| PAAS-04 | 部署执行 | agent 经本地 Docker(bollard)拉镜像/起容器/健康检查;日志与状态实时回流 | 3 |
| PAAS-05 | 域名绑定 | 自动分配 `应用名.企业域` 或自定义域名;网关路由自动生成;与灰度 upstream 联动 | 3 |
| PAAS-06 | 配置与密钥 | 按环境管理环境变量(版本化、变更留痕),敏感项加密存储、注入不落明文 | 3 |
| PAAS-07 | 证书自动申请与自动部署 🔌 | ACME(Let's Encrypt/ZeroSSL,含泛域名 DNS-01)申请/续期/到期提醒;经部署钩子自动部署到网关/LB/主机 | 3 |
| PAAS-08 | 发布单 | 关联版本(Tag/Release)、自动汇总变更(PR/工作项)、审批流、执行记录 | 4 |
| PAAS-09 | 灰度策略 | 滚动(逐实例)/分批(按比例)/蓝绿(主机组切换);暂停/继续/**一键回滚**(重放上一版规格) | 4 |
| PAAS-10 | 流量切换 | 灰度比例联动企业网关(Nginx upstream 权重 / APISIX traffic-split) | 4 |
| PAAS-11 | 变更日历 | 发布排期可视化、冲突检测、变更审计记录(ITIL) | 4 |
| PAAS-12 | 功能开关 | 用户/部门/白名单/百分比(稳定哈希分桶);生命周期管理(超期提醒回收);切换审计 | 4 |
| PAAS-13 | A/B 实验 🔌 | 集成 GrowthBook(私有化):开关联动、实验入口与结果页跳转 | 4 |
| PAAS-14 | 管控与合规 | 双面管控;生产发布与生产开关变更需审批、可配双人复核;错误率自动停批;全程审计 | 4 |
| PAAS-15 | 入口认证 | 应用一键接平台 SSO(OAuth2 反代),内网应用免开发获得统一登录 | 4 |
| PAAS-16 | 日志监控入口 与 资源伸缩 | 容器日志查看检索(可对接 Loki)、健康面板、IM 告警;CPU/内存限额、持久卷、手动副本数 | 4 |

### 2.6 效能度量(MS,3 项)

| ID | 功能 | 说明 | 批次 |
|---|---|---|---|
| MS-01 | DORA 四指标 | 部署频率、变更前置时间、变更失败率、MTTR;按团队/应用/时间下钻 | 3 |
| MS-02 | 效能报表 | PR 周期、评审响应、交付周期、缺陷逃逸;Excel 导出 | 3 |
| MS-03 | 外部链接器 | 工作项/PR/发布单页跳转 Grafana/Jaeger/SonarQube 等既有系统 | 3 |

### 2.7 搜索(SRCH,2 项)

| ID | 功能 | 说明 | 批次 |
|---|---|---|---|
| SRCH-01 | 全文搜索 | 工作项/Wiki(tantivy 嵌入式) | 2 |
| SRCH-02 | 代码搜索 | 跨仓库 n-gram 分词,push 事件驱动增量索引 | 3 |

### 2.8 开放能力与集成(OPEN,5 项)

| ID | 功能 | 说明 | 批次 |
|---|---|---|---|
| OPEN-01 | REST API | 全量 API + OpenAPI 文档(utoipa)+ 前端 TS client 自动生成 + 访问令牌;批次 1 起随域交付 | 1+ |
| OPEN-02 | IM 深度集成 | 企微/钉钉/飞书**卡片消息 + 机器人交互**:IM 内审批 PR、流转工作项、审批发布单 | 2 |
| OPEN-03 | MCP Server | 工作项/PR/代码/流水线暴露为工具,接企业内部 AI 助手 | 4 |
| OPEN-04 | AI 助手 🔌 | PR 摘要与风险提示、评审初筛、Issue 分类去重、自然语言查迭代进度;模型走企业网关,总开关可关 | 4 |
| OPEN-05 | IDE 插件 | VSCode/JetBrains:看工作项、建分支、发 PR、查流水线 | 4 |

### 2.9 用户体验(UX,2 项)

| ID | 功能 | 说明 | 批次 |
|---|---|---|---|
| UX-01 | 个人工作台 | 首页聚合:分配给我/我创建/@我/我的待办/我关注的 PR 与发布单 | 1 |
| UX-02 | 全局命令面板 | `Cmd+K`:全局搜索跳转(工作项/仓库/PR/人/页面) | 2 |

### 2.10 平台管理(ADMIN,4 项)

| ID | 功能 | 说明 | 批次 |
|---|---|---|---|
| ADMIN-01 | 系统管理 | 系统设置、用户与角色管理、仓库与应用配额 | 1 |
| ADMIN-02 | 备份恢复 | PG 逻辑备份 + 裸仓库目录 + 对象存储;备份脚本与恢复演练文档 | 1 |
| ADMIN-03 | 监控 | Prometheus 指标 + Grafana 面板模板、健康检查端点 | 1 |
| ADMIN-04 | 升级 | Compose 换镜像 + 版本化 migration,迁移遵循向后兼容 | 1 |

### 2.11 插件体系(EXT,1 项框架 + 官方插件分布上表)

| ID | 功能 | 说明 | 批次 |
|---|---|---|---|
| EXT-01 | 插件框架 | 扩展点契约:通知渠道 / 证书 Provider / 部署钩子 / CI Step·扫描器 / 事件订阅 / 权限点·自定义字段类型 / 前端扩展点(菜单·面板·工作项侧栏·卡片徽标) | 1 |

- 🔌 官方内置插件(经 EXT-01 扩展点实现):证书自动申请与自动部署(PAAS-07)、安全扫描器(CI-04)、A/B 集成(PAAS-13)、AI 助手(OPEN-04)、通知渠道(PM-13 / OPEN-02);
- **两级架构**:V1 内置插件(编译期,随平台发行)→ V2 动态插件(wasmtime + extism 沙箱、第三方、运行时安装、插件市场);
- **插件 Backlog(V2 候选,宝塔面板同类,均不进 V1)**:数据库实例管理(MySQL/PG)、计划任务管理、服务器资源监控面板、防火墙/端口管理、文件管理器。

---

## 3. 技术架构

### 3.1 总体形态

```
                    React SPA (TS, Vite, TanStack Query)
                        │  REST(OpenAPI) + WebSocket/SSE
┌───────────────────────▼────────────────────────────────────┐
│                Rust 内核(axum 模块化单体,可多实例)           │
│                                                            │
│  identity(用户/认证/RBAC/审计)│ project(工作项/迭代/看板/测试)│ wiki │
│  git-service(+片段/讨论) │ code-review(PR/门禁)          │
│  ci(流水线/扫描编排) │ release(发布单/灰度/开关)          │
│  insights(DORA/报表) │ appcenter │ search │ notify         │
│                                                            │
│  事件总线:NATS JetStream + PG 事务发件箱(outbox)             │
│  Git 传输:smart HTTP → git http-backend(CGI)│ SSH:russh  │
│  Git 读取:gitoxide(gix)      高亮:tree-sitter              │
└──────┬──────────┬──────────┬──────────┬────────────────────┘
       │          │          │          │
 PostgreSQL     Redis      NATS      MinIO/S3(经 Apache OpenDAL)
 (SQLx,可主从) (缓存/会话) (事件/队列)  (LFS/附件/制品/缓存)
       │
       ├─ Nginx 外部负载均衡(平台多实例横向扩展)
       ├─ Runner 容器池:bollard 调度 Docker(复用 GitHub Actions runner 二进制)
       └─ 部署目标主机群:每台一个 deploy-agent(Rust 单二进制,gRPC 主动外连注册,零入站端口)
            └─ agent 本地经 Docker API 拉镜像/起容器/健康检查,部署日志与状态实时回流
```

- **单体内聚**:模块间只走内部 API trait 与领域事件,不直连他模块表——未来拆分的天然切缝;
- **无 K8s 的横向扩展**:平台容器无状态,Compose 多实例 + Nginx 负载均衡;有状态件(PG/Redis/NATS/MinIO/裸仓库目录)独立主机或独立容器组;
- **灰度链路**:发布单 → 状态机 → 引擎分批换实例镜像 + 网关流量权重调整 → 批次结果事件回写发布单 → 联动工作项。

### 3.2 代码仓库结构(Rust workspace + 前端 monorepo)

> 解耦规则、crate 依赖约束、公共方法清单、前端技术栈与 UI 设计方向详见 [ARCHITECTURE.md](./ARCHITECTURE.md)。

```
codeplan/
├─ crates/
│  ├─ kernel/        # 身份、RBAC、审计、配置、错误、事件契约
│  ├─ identity/      # 身份域:用户/组织/团队、本地账号、JWT 会话、RBAC、审计
│  ├─ project/       # 工作项、迭代、看板、工时、测试管理(域逻辑)
│  ├─ wiki/
│  ├─ git-service/   # 仓库生命周期、传输代理(gix 读 / http-backend 写)、浏览、LFS、代码片段、讨论区
│  ├─ code-review/   # PR、评审、分支保护、CODEOWNERS 解析与指派
│  ├─ ci/            # workflow 解析、runner 调度(bollard)、制品库、扫描编排
│  ├─ release/       # 发布单、灰度策略状态机、部署引擎编排(期望状态调和/批次推进/回滚)、变更日历
│  ├─ agent/         # 部署目标主机上的轻量 agent(独立二进制):gRPC 外连、本地 bollard 执行、状态与日志上报
│  ├─ flags/         # 功能开关:规则引擎、放量计划、分发 API、切换审计
│  ├─ appcenter/     # 应用中心:应用聚合视图、环境登记
│  ├─ insights/      # DORA 指标计算、效能报表、外部链接器
│  ├─ search/        # tantivy 索引(工作项/代码)
│  ├─ notify/        # 站内信/邮件/IM 卡片与机器人交互
│  ├─ eventbus/      # outbox dispatcher、NATS 封装、事件 schema
│  ├─ storage/       # OpenDAL 封装
│  ├─ ai/            # MCP Server、AI 助手(评审摘要/分类/问答)
│  ├─ common/        # 纯工具库:统一响应/分页/错误码/强类型ID/时间/校验/测试工具(无业务语义)
│  ├─ server/        # 组合根(唯一二进制):装配各域路由/事件订阅/插件,聚合导出 OpenAPI
│  └─ ext/           # 插件 SDK:扩展点 trait 契约、插件注册与生命周期
├─ plugins/          # V1 官方内置插件:cert-acme(证书自动申请与自动部署)、notify、scanner、growthbook、ai——与内核同编译发行
├─ migrations/       # sqlx 迁移
├─ web/              # pnpm + Vite monorepo(内核 shell + 各域前端包)
├─ integrations/     # IDE 插件(VSCode/JetBrains)、IM 机器人适配器
├─ deploy/           # docker-compose(平台/全套依赖)、Nginx 配置模板、备份脚本
└─ docs/
```

### 3.3 技术栈清单

| 层 | 选型 |
|---|---|
| 运行时/Web | tokio + axum + tower-http |
| 数据库 | PostgreSQL 16 + SQLx(编译期校验)+ sqlx migrate;主从可选 |
| API 契约 | utoipa → OpenAPI → 前端 TS client |
| Git 读取 | gitoxide(gix);tree-sitter 高亮;imara-diff/similar |
| Git 写入 | git 官方 `http-backend`(CGI 代理)+ `receive-pack`;pre/post-receive hook 发事件 |
| SSH | russh(纯 Rust)或系统 sshd forced-command |
| 签名验证 | sequoia-openpgp + ssh-key |
| 搜索 | tantivy(嵌入式;代码 n-gram 分词) |
| 事件 | NATS JetStream(async-nats)+ PG 事务发件箱 |
| 任务调度 | apalis(PG 后端)+ tokio-cron-scheduler |
| 对象存储 | Apache OpenDAL(S3/MinIO/OSS/COS 统一) |
| 容器操作 | **bollard(Docker API)**:平台 runner 调度 + agent 在目标主机本地执行——无 K8s |
| 部署引擎通道 | tonic(gRPC 双向流:注册/指令下发/状态心跳)+ HTTP(日志与产物回流) |
| 网关与域名 | APISIX(Admin API)或 Nginx(平台生成配置 + reload):灰度权重、域名路由、泛域名证书;入口认证走 OAuth2 反代 |
| 认证 | openidconnect、ldap3、jsonwebtoken、argon2、totp-rs |
| 缓存 | Redis(redis-rs) |
| 安全扫描 | Semgrep / gitleaks / trivy(容器执行,结果进检查门禁) |
| 实验平台 | GrowthBook(私有化集成,不自研) |
| 可观测 | tracing + OpenTelemetry OTLP;Prometheus 指标 |
| 通知 | lettre(邮件);企微/钉钉/飞书卡片与机器人(reqwest) |
| AI | MCP 协议;模型走企业内部网关(OpenAI 兼容接口) |
| 插件 | V1 编译期扩展点(crates/ext);V2 预留 wasmtime + extism |
| 前端 | React + TS + Vite + TanStack Query/Router,pnpm + Turborepo,shadcn/ui(Radix + Tailwind)+ TanStack Table,AI UI 用 Vercel AI SDK + assistant-ui |

### 3.4 关键设计

**Git 传输路径(自研层的核心组装)**
- clone/push:按仓库路径鉴权后拉起 git `http-backend` CGI 完成协议交换;认证走内部令牌;
- push 后:`post-receive` hook 调内部 API → 发 `commit.pushed` 事件 → 驱动关联/索引/通知/PR 检查;
- 读路径全走 gix,`GitRepo` trait + adapter 封装(锁版本,gitoxide 未来支持 push 后可切纯 Rust)。

**部署引擎(agent 模型)**
- 目标主机上的 agent 主动外连平台(gRPC 长连注册,Token 认证,目标主机零入站端口);平台持有"期望状态"(版本化服务规格),agent 将其调和到本地 Docker;健康检查通过才推进下一批次,失败自动停批;回滚 = 重放上一版本规格;平台自身与用户应用统一由该引擎部署(吃自己的狗粮)。

**灰度/AB 双面管控模型**
- 实例面(灰度发布)控制"新版本代码到多少实例、多少流量"——发布单状态机 + 分批健康门禁 + 网关权重;用户面(开关/实验)控制"功能对哪些用户生效"——稳定哈希分桶(同一用户恒定同组)+ SDK 下发 + GrowthBook 分析。两面协同:先灰度少量实例,再经开关对内部用户开放新功能,观察指标后逐级放量,各自独立可回退。

**工作项 ↔ 代码 ↔ 发布关联(产品灵魂)**
- 关联表 `work_item_ref`:work_item_id / ref_type(commit|pr|branch|release|deployment)/ ref_key;
- 联动链:commit `#ID` → PR 合并 → 发布单上线 → 工作项状态自动流转"已上线",全事件驱动、规则可配置。

**事件体系**
- 业务写入与 outbox 同事务;dispatcher 发 NATS(at-least-once,消费方幂等);
- 核心事件:`workitem.*`、`pr.*`、`commit.pushed`、`branch.*`、`tag.created`、`pipeline.{started,finished}`、`scan.{completed,findings}`、`release.{created,approved,started,batch_done,rolled_back,finished}`、`flag.{created,toggled}`、`app.{created,updated,domain_bound,config_changed}`、`member.{added,removed}`。

**权限模型**
- RBAC 三层(系统角色 → 项目角色 → 仓库 ACL)+ 分支保护门禁;全部资源访问经统一 `Authz` trait,审计同层埋点。

**部署形态(无 K8s)**
- 交付:`deploy/docker-compose.yml` 一套起全(PG/Redis/NATS/MinIO/平台 N 实例/Nginx);
- 扩展:平台容器无状态,加实例 + Nginx upstream 即横向扩展;
- 升级:新镜像 + 版本化 migration;回滚:镜像回退(平台侧),数据迁移遵循向后兼容原则。

**非功能**
- 性能:常用页面 P95 < 300ms;Git HTTP 吞吐 ≥ git 裸服务 90%;
- 安全:内网 TLS、全量审计、cargo-audit + cargo-deny 进自身 CI、密钥配置注入不入库;
- 可观测:trace_id 贯穿 Web → Git 子进程 → runner → 事件消费;业务指标(PR 周期/发布成功率)内建为效能度量的数据源。

---

## 4. 核心数据模型(主要实体)

```
User ─┬─ Team ─ Project ─┬─ WorkItem(类型/状态机/自定义字段)─┬─ WorkItemRef ─► (commit|pr|branch|release)
      │                  ├─ Sprint        ├─ Comment          ├─ Board/BoardColumn
      │                  ├─ Milestone     ├─ TestCase(测试管理)└─ Worklog
      │                  └─ WikiPage(树)  └─ Attachment
      └─ Role / AccessToken / SshKey

App(应用)─┬─ Repository ─┬─ PullRequest ─ Review/ReviewComment/Check(含扫描门禁)
           │              ├─ CodeOwnerRule / Webhook / Discussion / Snippet
           ├─ Pipeline ─ PipelineRun ─ Step ─ Artifact ─ 制品库
           ├─ Environment ─ Release(发布单)─ DeployBatch ─ RollbackRecord
           ├─ ServiceSpec(版本化)─ Deployment ─ InstanceReport
           ├─ DomainBinding(域名/路由)─ 证书
           ├─ AppConfig(环境变量/密钥,版本化)
           └─ DeployHost(部署目标主机)─ agent 心跳/标签

EventOutbox / AuditLog / DoraMetric(事件聚合事实表) / ExternalLink(外部链接器)
```

---

## 5. 构建批次(V1 内部推进顺序,批次四完成 = V1.0 发布)

| 批次 | 内容(功能项) | 出口标准 |
|---|---|---|
| **批次一 · 基座**(23 项) | 组织/权限/OIDC/LDAP(ORG-01~03,05,07)、工作项/状态机/迭代/看板/树形/缺陷(PM-01~05,08)、Wiki(PM-11)、事件总线、审计(ORG-05)、个人工作台(UX-01)、平台管理(ADMIN-01~04)、插件框架基础(EXT-01,站内通知作为首个官方插件验证)、Git 基础(GIT-01,02,04)、API(OPEN-01) | 10 人团队在平台上跑完一个完整迭代;代码可 clone/push;一个官方插件经扩展点接入成功 |
| **批次二 · 协作**(15 项) | SSH/LFS(GIT-03,08)、PR/评审/分支保护/CODEOWNERS(GIT-05,06,09)、`#ID` 关联与合并联动(GIT-10)、Tag/Release(GIT-07)、Webhook(GIT-11)、访问凭证(ORG-04)、IM 卡片交互(OPEN-02)、报表 + 导出(PM-12)、全文搜索(SRCH-01)、命令面板(UX-02)、甘特图(PM-06)、需求池(PM-07) | 需求→分支→PR→评审→合并→工作项自动流转全闭环;域账号登录;在 IM 里完成一次 PR 审批 |
| **批次三 · 流水线**(17 项) | CI pipeline + runner(CI-01,02,05)、制品库(CI-03)、扫描门禁(CI-04)、代码搜索(SRCH-02)、工时(PM-09)、DORA 与效能报表(MS-01,02)、应用中心与外部链接器(PAAS-01,MS-03)、**部署引擎基础**(PAAS-02~06)、证书插件(PAAS-07) | 提交触发流水线、扫描卡住不合规 PR;DORA 四指标出数;应用"一键部署"到目标主机并经绑定域名(证书自动签发)健康上线 |
| **批次四 · 发布与智能**(16 项) | 发布单/灰度/蓝绿/回滚/变更日历(PAAS-08~11)、管控与合规(PAAS-14,ORG-06)、功能开关 + GrowthBook(PAAS-12,13)、入口认证(PAAS-15)、日志监控入口与资源伸缩(PAAS-16)、测试管理(PM-10)、AI 助手/MCP(OPEN-03,04)、IDE 插件(OPEN-05)、代码片段/讨论区(GIT-12,13) → **V1.0 GA** | 一次真实灰度发布(分批换镜像 + 网关切流 + 一键回滚)全程在平台完成;生产开关双人复核与超期回收生效;AI 评审摘要可用 |

---

## 6. 风险与对策

| 风险 | 对策 |
|---|---|
| V1 范围大(71 项),交付周期长 | 批次一~四严格串行、每批次出口可运行可演示;中途进度失控时砍单顺序已定:先砍智能类(OPEN-04/05、GIT-12/13),发布域(PAAS)不动 |
| gix 仍处 0.x,API 变动 | `GitRepo` trait + adapter 封装,锁版本,升级只改 adapter |
| Rust 迭代速度/招聘 | 紧贴 axum+SQLx 成熟组合,不过度抽象;模块边界清晰便于分工 |
| PR/评审域复杂度高 | 批次二内部分两步:先合并能力,后评审流与门禁 |
| 自研部署引擎范围失控 | 严格限定"装 Docker 的主机上的长驻服务":无虚拟机/裸金属目标、无定时任务编排(归 CI)、无多云、V1 无自动弹性;策略状态机保持最小集(滚动/分批/蓝绿三种);agent 保持无状态,全部状态在平台侧 |
| Actions 兼容范围失控 | 明示支持的语法子集清单,超出即报错,不做全兼容承诺 |
| IM 卡片交互多平台适配量 | 抽象统一卡片/回调协议,三个 IM 各一个薄适配器;先企微后钉钉/飞书 |
| AI 能力依赖外部模型 | 全部 AI 功能带总开关,模型走企业内部网关,不可用时平台核心功能零依赖 |
