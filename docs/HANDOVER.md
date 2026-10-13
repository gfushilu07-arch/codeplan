# CodePlan 项目交接文档

> 面向接手者的第一份文档。读完本文 + [PLAN.md](../PLAN.md)(功能基线)+ [ARCHITECTURE.md](../ARCHITECTURE.md)(架构规约)即可开工。
> 进度以 [TASKS.md](../TASKS.md) 为准,本文只给全局视图与踩坑清单。
> 最后更新:2026-10-13。

## 1. 这是什么,做到哪了

**CodePlan** —— 企业内部一体化研发平台:项目管理(对标 Jira/禅道)+ 代码托管(对标 GitHub/Gitee)+ 内部 PaaS 部署(自研部署引擎,**不用 K8s**)。单租户私有部署,Rust 后端 + React 前端,官方功能以插件形式接入。

| 阶段 | 任务数 | 已完成 | 状态 |
|---|---|---|---|
| 阶段 0 · 工程基建 | 7 | 7 | ✅ |
| 批次一 · 基座(组织/权限/项目/工作项/Wiki/Git) | 25 | 25 | ✅ |
| 批次二 · 协作(SSH/PR/评审/Release/Webhook/搜索/报表) | 16 | 16 | ✅ |
| 批次三 · 流水线(CI/制品/扫描/DORA/部署引擎/证书) | 17 | 16 | 🔄 仅余 T-314 |
| 批次四 · 发布与智能(发布单/灰度/回滚/开关/测试/AI) | 20 | 0 | ⬜ 未开始 |
| **合计** | **85** | **64** | — |

规模:后端 **20 个 crate** + **2 个官方插件**,**37 个数据库迁移**,前端 **17 个 workspace 项目**(1 应用 + 9 个 feature 包 + 7 个基础包;其中 `features/deploy`、`features/pipeline` 目前是空壳),**203 项 Rust 测试全绿**。

**已经真跑通的闭环**(不是模拟):
1. **一键部署**:提交服务规格 → 平台生成容器计划(配置解密后注入)→ gRPC 下发 agent → agent 拉镜像起容器 → 健康检查 → 状态回写 `success`。
2. **域名 → HTTPS**:绑定域名 → ACME(pebble/DNS-01)签发真实证书 → 落盘 → 回填 `cert_ref` → 网关自动启用 443 → `nginx -t` 通过。
3. **主机纳管**:agent 主动外连注册(主机侧零入站端口)→ 心跳 → 平台判定在线。
4. **研发协作**:需求 → 分支 → PR → 评审 → 合并 → 工作项自动流转;push 触发流水线;扫描不合格卡住合并。

---

## 2. 五分钟跑起来

### 前置

Rust 工具链、Node 20+/pnpm 10、Docker(Docker Desktop 或 colima)。

> **注意**:本机 Rust 工具链装在外部卷 —— `CARGO_HOME=/Volumes/fushilu/.cargo`、`RUSTUP_HOME=/Volumes/fushilu/.rustup`。`cargo` 不在默认 PATH 里,需要:
> ```bash
> export CARGO_HOME=/Volumes/fushilu/.cargo
> export RUSTUP_HOME=/Volumes/fushilu/.rustup
> export PATH="$CARGO_HOME/bin:$PATH"
> ```
> 换机器接手时改成自己的路径即可,`~/.zshenv` 里有原始配置。

### 起依赖(Postgres / Redis / NATS)

```bash
docker compose -f deploy/docker-compose.yml up -d
# PG 5434、Redis 6381、NATS 4223(监控 8223)
```

### 起后端

```bash
cargo build -p codeplan-server
CODEPLAN_ADDR=0.0.0.0:8081 \
CODEPLAN_PUBLIC_URL=http://localhost:8081 \
CODEPLAN_GRPC_ADDR=0.0.0.0:50051 \
./target/debug/codeplan-server
```

**`CODEPLAN_PUBLIC_URL` 必须设对**。git 钩子靠它回连平台,默认值(`http://localhost:8080`)是错的,会让所有 push 失败(见 §6.6)。

首次启动自动跑迁移、建管理员账号(`admin` / `codeplan-admin`,可用 `CODEPLAN_BOOTSTRAP_ADMIN_PASSWORD` 改)、重建搜索索引。

### 起前端

```bash
cd web && pnpm install && pnpm --filter @codeplan/main dev
# http://localhost:5173
```

### 起 agent(要试部署时)

```bash
# 1) 平台登记主机,拿到一次性令牌
TOKEN=$(curl -s -X POST http://localhost:8081/api/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"codeplan-admin"}' | jq -r .data.token)
curl -s -X POST http://localhost:8081/api/deploy-hosts \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"name":"host-1","labels":["linux","docker"]}' | jq -r .data.register_token

# 2) 在目标主机上跑 agent(令牌只能用一次)
CODEPLAN_AGENT_SERVER=http://<平台地址>:50051 \
CODEPLAN_AGENT_TOKEN=<上一步的令牌> \
CODEPLAN_AGENT_LABELS=linux,docker \
./target/debug/codeplan-agent
```

### 端口一览

| 端口 | 用途 |
|---|---|
| 8081 | 后端 HTTP API(**不是 8080**,那个常被其他服务占用) |
| 50051 | agent gRPC 网关 |
| 5173 | 前端 dev server |
| 2322 | Git SSH(**不是 22**,避免与本机 sshd 冲突) |
| 5434 / 6381 / 4223 | PG / Redis / NATS(刻意避开默认端口) |
| 14000 / 8055 | pebble ACME 测试 CA / challtestsrv(仅本地验证用) |

---

## 3. 架构地图

### 后端:四层,依赖方向单向

```
L1  common(纯工具,禁业务概念)   kernel(平台契约:trait/ID/事件 schema)
L2  eventbus  storage  ext(插件 SDK)  notify  agent
L3  业务域:identity project wiki git-service code-review ci release
           flags appcenter insights search ai
L4  server(组合根:唯一允许依赖一切的地方)
```

**分层不是摆设**:`cargo xtask lint-deps` 会强制校验白名单,越界直接失败。跨域协作只有两条路 —— **领域事件**(在 `kernel::events` 定义 schema)或 **kernel trait**(`Authz`/`IdentityLookup`/`AuditSink`/`GitRepoLookup`)。**禁止 L3 之间互相 `use`,禁止 SQL 查别的域的表**(表名带域前缀:`proj_`/`repo_`/`rev_`/`ci_`/`app_`/`ins_`/`org_`,前缀即所有权)。

> 例外说明:CI/appcenter/insights 里确实有少量对其他域表的**只读投影**查询(定位仓库、跨域统计)。这是既有做法,写入权始终归归属域;新增此类查询请在 PR 里说明理由。

### 前端:feature 包与基础包分离

```
web/
├── apps/main                 应用壳(路由、Shell、ErrorBoundary)
├── features/                 业务页面,彼此禁止互相 import
│   ├── workitem  repo  wiki  admin  insights  search  appcenter
│   └── deploy / pipeline     ← 目前是空壳,批次四填
└── packages/                 基础能力
    ├── design-tokens         设计令牌唯一来源(色/间距/圆角)
    ├── ui  shared  icons     基础组件
    ├── api-client            接口封装(生成物,禁手改)
    ├── events                事件类型
    └── ai-ui                 AI 流式/生成式卡片
```

### 插件

`crates/ext` 是插件 SDK(扩展点 trait + 注册表),官方插件是**编译期内置**、随平台发行:

| 插件 | 扩展点 | 作用 |
|---|---|---|
| `plugins/inbox` | `NotificationChannel` | 站内信通知 |
| `plugins/cert-acme` | `CertIssuer` | ACME 签发证书(DNS-01) |

新增插件:`plugins/<name>/` 实现 `ext` 里某个 trait → 组合根装配 → **在 `xtask/src/lint_deps.rs` 的 L4 白名单登记**(忘了登记会被 lint-deps 拦下)。

---

## 4. 铁律(违反 = PR 必拒)

1. **依赖方向**:L1←L2←L3←L4,禁止反向;禁止 L3 之间互相依赖。加跨 crate 依赖前跑 `cargo xtask lint-deps`。
2. **跨域只有事件或 kernel trait 两条路**;禁止跨域实体引用;禁止跨域写 SQL。
3. **`common` 不准出现业务概念**(User/Project/Issue/PR…),不准依赖本项目任何 crate。
4. **前端 `features/*` 之间禁止互相 import**;`packages/*` 不得 import `features/*`。
5. **Git 写路径只准走官方 `http-backend`/`receive-pack` 代理;读路径只准经 `GitRepo` adapter**;**禁止引入 git2/libgit2 等 C FFI**(当前读路径是 git CLI porcelain)。
6. **所有端点带 utoipa 注解**;错误码 `<域>-<3位>`(如 `GIT-404`)且必须在所属域的错误枚举注册,不准就地 `anyhow` 返给前端。
7. **迁移向后兼容**:上一个版本的二进制必须能在新库上跑(只加不减)。

### 公共代码怎么放(写之前必读)

先问:**这段逻辑的第二个真实调用者在哪里?** 答不出来就地放本 crate 的 `support/`,别动公共层。

```
只在当前 crate 内复用          → 本 crate 的 support/
2 个以上 crate 且是纯工具       → common/<具名模块>.rs(禁 utils.rs 垃圾抽屉)
涉及平台契约(ID/权限/审计/事件) → kernel
是基础设施访问模式              → 扩 eventbus/storage/search 既有接口
```

**提升方向只有一个**:crate 内 `support/` →(出现第二个调用者)→ `common`/`kernel`。禁止"以后可能用到"的提前抽象。

---

## 5. 关键机制

### 事件

NATS JetStream + **PG 事务外盒**(`event_outbox`,保证不丢)。`EventEnvelope { id, type, occurred_at, actor, payload }`。NATS 消息里的 `payload` 是**嵌套**的 —— 消费端取值要同时兼容嵌套与顶层(现有代码里普遍有个 `field()` 小助手,照抄)。

订阅方必须**幂等**(at-least-once)。当前订阅者:identity(repo/project/app.created 授权)、project(commit.pushed/pr.merged)、ci(push 触发/状态回写/扫描门禁)、search(索引增量)、notify、webhook、insights(DORA)。

### 鉴权

- 域内不依赖 identity:`kernel::TokenAuthenticator` 中间件把 `CurrentUser` 注入请求扩展。
- 资源级授权经 `kernel::authz::Authz`,资源定位 `ResourceLocator::{System,Project,Repo,App}`。
- 创建资源后由 identity 监听 `*.created` 事件自动授予创建者 admin。
- agent / git 传输走独立凭据(一次性注册令牌 / Basic 密码位放 JWT)。

### 数据库

`sqlx` + `migrations/`(37 个,只增不改)。**迁移文件的 checksum 是 SHA-384**,改已应用的迁移会 `VersionMismatch` 拒绝启动 —— 需要改就新增迁移。重复执行要写幂等写法(`CREATE TABLE IF NOT EXISTS` / `ON CONFLICT`)。

### 部署引擎(批次三建成)

```
应用(appcenter)→ 环境 → 部署目标(登记) → 服务规格(版本化)→ 配置与密钥(加密)
                                                     ↓
                                        一键部署:生成容器计划 + 注入配置
                                                     ↓
                                    gRPC 下发(agent 主动外连,零入站端口)
                                                     ↓
                                        agent 本机 bollard 拉镜像/起容器/健康检查
                                                     ↓
                                        回执 → 部署状态汇总 → WebSocket 回流前端
```

域名与证书:域名绑定 → 网关路由生成(Nginx 配置 / APISIX 路由)→ ACME 签发 → 回填 `cert_ref` → 网关 TLS 分支自动生效。

---

## 6. 踩坑清单(都是实际撞过的)

### 6.1 api-client 信封陷阱(已踩 4 次)

后端所有响应都包在 `{code,message,data}` 里,而 `api-client` 的 `request<T>()` **不解包**。漏解包 → `setState(整个信封)` → 渲染期 `.map`/`.slice` 抛错 → **React 19 卸载整棵树 → 全站白屏**,且 root 为空、无报错文本,极易误判成样式问题。

**判定白屏是否属于这类**:浏览器里看 `document.getElementById('root').children.length`,为 0 就是渲染崩溃,不是 CSS。

**新写 api-client 函数时**:
```ts
const payload = await request<{ data?: T }>(...);
return payload.data ?? (payload as unknown as T);
```
页面已有 `ErrorBoundary`(`web/apps/main/src/ErrorBoundary.tsx`,App 里按 route 作 key)兜底,单页异常只降级内容区。

### 6.2 `git ls-tree -l` 字段顺序

输出是 `<mode> <type> <oid> <size>\t<name>`。少读一个字段会把 mode 当 kind、type 当 oid —— 表现为**目录全显示成文件、大小恒空、oid 恒为 `"blob"`**。另外 `--name-only` 与 `-l` **不能同用**(会直接报错)。

### 6.3 Docker 端口协议

规格里的协议是**应用层**的 `http`/`grpc`,Docker 端口只认 `tcp`/`udp`。直接透传会让 `docker start` 报 `unknown protocol`,容器起不来。必须归一化(见 `agent/src/executor.rs::docker_protocol`)。

### 6.4 rustls 未选定 provider → 首次 TLS 握手 panic

依赖树里同时有 aws-lc-rs 与 ring 时,rustls 无法自动选。症状是**请求瞬间返回空响应**(不是超时)。修法:启动时显式 `install_crypto_provider()`(见 `plugins/cert-acme`)。

### 6.5 悬空状态

部署下发后主机掉线 → 回执永不到达 → 部署永远 `pending`。**运维工具不能有悬空状态**,已有 5 分钟超时收敛。同类问题的检查点:任何"等外部回执"的状态机都要有超时。

另:主机"在线判定"有 3× 心跳间隔宽限,宽限期内**已死的主机仍算在线** —— 部署选目标用的是更严的"心跳新鲜"(1 个间隔内),别把两者混用。

### 6.6 git push 被拒时报错会骗人

pre-receive 钩子回连平台失败时,原来会打印 `push rejected by branch protection` —— **和分支保护毫无关系**。现已区分「无法回连钩子端点」并提示检查 `CODEPLAN_PUBLIC_URL`。**看到 push 被拒先看是不是 8080/8081 配错**。

### 6.7 测试数统计本身会骗人

用 `awk` 拼统计时格式一变就把"有测试失败"算成"全过"。**统计口径要单独核对**(建议直接看 `test result: FAILED` 出现与否)。

### 6.8 认证相关

- `/api/me`、`/api/admin/*` 等需要 Bearer 令牌;令牌过期会让脚本中途失败,长脚本记得中途重新登录。
- WebSocket 无法从浏览器设请求头,部署观测用 **`?token=` 查询参数**鉴权。

### 6.9 空壳不等于已实现

`crates/{agent,appcenter,flags,release,ai}` 曾经都是 3 行占位;`web/features/{deploy,pipeline}` **现在仍是空壳**。接手后看到目录存在**不代表功能存在**,以 TASKS.md 勾选状态为准。

---

## 7. 测试与门禁

```bash
# 后端(DoD 要求四条全绿)
cargo clippy --workspace -- -D warnings
cargo test --workspace                  # 203 项
cargo run -p xtask -- lint-deps         # 23 个 crate 白名单
cargo run -p xtask -- sync-rules        # AGENTS.md → CLAUDE.md 等生成物同步

# 前端
cd web && pnpm build && pnpm lint && pnpm test   # 17 个包
```

当前状态:**全部通过**。CI 配置在 `.github/workflows/ci.yml`(后端六步 + 前端三步)。

`-D warnings` 是硬门禁,连死代码都会拦(未使用的 `pub fn` 也算),**不要靠 `#[allow]` 绕过**,把代码删掉或接上真实调用。

---

## 8. 未完成工作(21 项)

### 批次三余 1 项

- **T-314 部署可观测** —— 核心链路**已实测通过**,但有一处**未解决的性能问题**。
  - **已验证**:WebSocket 收到 22 条事件(2 条状态 + 20 行真实 nginx 启动日志);鉴权正确(伪造令牌 401、有效令牌 101 升级)。
  - ⚠️ **未解决**:部署**回执到达平台约需 24 秒**,导致部署状态被人为延迟。已排除"日志跟随阻塞主流程"(改到后台无效),**真因未定位**。建议接手后在 agent 的 `execute()` 各阶段打点(拉镜像 / 建容器 / 启容器 / 健康检查)逐一计时 —— 最可疑的是 `create_image` 每次都会联系 registry 校对新 digest。
  - **待做**:修上述延迟;前端「部署记录 + 实时日志面板」;发布进度可视化数据结构。
  - 注意:`follow_logs` 只在部署执行期内跟随(上限 30 秒),容器长期运行期的日志订阅需另行设计。

### 批次四 20 项(整批未开始)

- **发布域 6 项**:发布单+审批流、灰度策略状态机(滚动/分批/蓝绿+健康门禁+错误率自动停批)、一键回滚、流量切换(网关权重联动)、变更日历、生产双人复核与合规审计
- **开关与部署 6 项**:flags 规则引擎(用户/部门/白名单/百分比稳定哈希)+ 多语言 SDK、开关生命周期、GrowthBook 集成插件、应用入口 SSO(OAuth2 反代)、容器日志检索+健康面板+IM 告警、资源限额/持久卷/副本数
- **测试与智能 8 项**:测试管理(用例库/计划/执行关联缺陷/报告)、MCP Server、AI 助手插件(PR 摘要/评审初筛/Issue 分类)、前端 AI UI(聊天侧栏+流式+生成式卡片)、VSCode/JetBrains 插件、代码片段+讨论区、审计合规报表(PDF/Excel)、前端发布单/灰度可视化

**建议顺序**:T-314 收尾 → 发布单与灰度(T-401/402/403/404,它们把批次三已建成的部署能力串成"可管控的发布")→ flags(T-410/411)→ 其余。发布域依赖的技术底座(规格/配置/证书/网关/部署执行)都已就绪。

---

## 9. 已知偏差与风险

**已交付但范围收窄(3 处,均已在 TASKS.md 对应条目注明)**:

1. **T-004**:标题含"OpenAPI diff 检查",实现是后端六步 + 前端三步,**未纳入 diff 校验**。
2. **T-113**:看板/树形已做,**列表视图的筛选/批量未实现**。
3. **T-305**:扫描能力落在 `crates/ci` 而非经插件扩展点接入(`ext` 的「CI step」仍是 trait 占位);**扫描结果也没有前端展示页**。T-316/T-311/T-313 都走了正规插件/扩展点路径,可作为迁移参照。

**风险点**:

- **敏感配置主密钥**:未配 `CODEPLAN_CONFIG_KEY` 时用固定开发密钥并启动告警。**生产必须显式配置**,否则换密钥后旧密文不可解。
- **DNS-01 下发**:默认可插拔(文件 / HTTP 端点)。生产接入真实 DNS 厂商 API 时需要自己实现 `DnsSolver`(`plugins/cert-acme/src/lib.rs`)。
- **APISIX 推送**:目前只生成路由体 + Admin API 路径,由运维落地;`apply_gateway` 走的是写本地 Nginx 配置目录。
- **agent 与平台同机**(当前开发环境如此):多 agent 会争抢同名容器。生产每台主机一个 agent 则无此问题。
- **凭据落库**:部署时配置与密钥以**明文经 gRPC** 下发 agent(不落库、不经 HTTP),agent 不持久化。若威胁模型要求更严,需在 agent 侧加内存零化。

**环境残留物**(开发机,可清):`admin/scan-demo2` 等测试仓库;`web-portal`/`ui-demo-app` 应用及其环境/目标/变量/规格/配置/域名/证书;运行中的 `codeplan-web-portal-dev-1` 容器(nginx);5 台登记主机;`/tmp/cp-gateway`、`/tmp/cp-certs`;pebble + challtestsrv 容器。

---

## 10. 环境变量速查

| 变量 | 用途 | 默认 |
|---|---|---|
| `CODEPLAN_ADDR` | 后端监听 | `0.0.0.0:8080` ⚠️ **改用 8081** |
| `CODEPLAN_DATABASE_URL` | PG 连接 | `postgres://codeplan:codeplan-dev@localhost:5434/codeplan` |
| `CODEPLAN_NATS_URL` | NATS | `nats://localhost:4223` |
| `CODEPLAN_PUBLIC_URL` | 对外地址(**git 钩子回连用**) | `http://localhost:8080` ⚠️ **必须改** |
| `CODEPLAN_DATA_DIR` | 数据根(仓库/附件/索引/证书) | `./data` |
| `CODEPLAN_JWT_SECRET` / `CODEPLAN_HOOK_SECRET` | 会话 / 钩子密钥 | 开发默认值 |
| `CODEPLAN_BOOTSTRAP_ADMIN_PASSWORD` | 首次建管理员 | `codeplan-admin` |
| `CODEPLAN_SSH_PORT` | Git SSH | `2322` |
| `CODEPLAN_GRPC_ADDR` | agent gRPC 网关 | `0.0.0.0:50051` |
| `CODEPLAN_CONFIG_KEY` | **敏感配置主密钥**(base64 32 字节) | 无(退化开发密钥) |
| `CODEPLAN_RUNNER_LABELS` | CI 内置 runner 标签 | `ubuntu-latest,shell` |
| `CODEPLAN_GATEWAY_CONF_DIR` | 网关闭配置落地目录 | 无(未配则拒绝落地) |
| `CODEPLAN_GATEWAY_CERT_DIR` | 网关证书目录 | `./data/certs` |
| `CODEPLAN_APP_DOMAIN_SUFFIX` | 自动分配域名后缀 | `apps.internal` |
| `CODEPLAN_ACME_DIRECTORY` | ACME 目录 URL(**不配则不启用证书插件**) | 无 |
| `CODEPLAN_ACME_CA_ROOT` | 自定义 CA 根(测试/私有 CA) | 无(用系统信任库) |
| `CODEPLAN_ACME_DNS_API` / `_DNS_FILE` / `_DNS_TOKEN` | DNS-01 下发方式 | 文件 |
| `CODEPLAN_ACME_ACCOUNT_KEY` | ACME 账号凭据路径 | `<data>/acme/account.json` |
| `CODEPLAN_ACME_PROPAGATION_WAIT` | DNS 传播等待秒 | 5 |
| `CODEPLAN_AGENT_SERVER` / `_TOKEN` / `_LABELS` | **agent 侧**:平台地址 / 一次性令牌 / 标签 | — |
| `CODEPLAN_OIDC_*` | OIDC 单点登录(issuer/client/secret/redirect) | 无 |
| `CODEPLAN_LDAP_*` | LDAP/AD 同步与认证 | 无 |

---

## 11. 本地验证 ACME 的方法(批次三用过的)

```bash
# 官方测试 CA + 挑战测试服务
docker run -d --name pebble-challtestsrv --network cp-acme -p 8053:8053/udp -p 8055:8055 \
  ghcr.io/letsencrypt/pebble-challtestsrv:latest -dnsserver :8053 -management :8055
docker run -d --name pebble --network cp-acme -p 14000:14000 \
  ghcr.io/letsencrypt/pebble:latest \
  -config /test/config/pebble-config.json -dnsserver pebble-challtestsrv:8053
docker cp pebble:/test/certs/pebble.minica.pem /tmp/cp-certs/pebble.minica.pem

# 后端带 ACME 配置启动
CODEPLAN_ACME_DIRECTORY=https://localhost:14000/dir \
CODEPLAN_ACME_CA_ROOT=/tmp/cp-certs/pebble.minica.pem \
CODEPLAN_ACME_DNS_API=http://localhost:8055 \
CODEPLAN_ACME_PROPAGATION_WAIT=2 ...
```

注意 pebble 的 flag 是 `-dnsserver`(**不是 `-dns01`**),写错会直接退出。

Nginx 配置合法性可以用容器校验(不需要本机装 nginx):

```bash
docker run --rm -v /tmp/cp-gateway:/etc/nginx/conf.d:ro \
  -v /tmp/cp-certs:/etc/codeplan/certs:ro nginx:alpine nginx -t
```

---

## 12. 接手后建议的第一周

1. 按 §2 把整套跑起来,确认 203 项测试与前端构建全绿。
2. 通读 `TASKS.md` 的批次三部分 + 本文 §5,理解部署与证书链路。
3. 做 **T-314 的端到端验证**(代码已就位,只差实测),然后勾选收尾批次三。
4. 进批次四前先读 `PLAN.md` 的 PAAS-08~14(发布单/灰度/回滚/流量切换/合规),它们是同一串能力,建议一起规划。
