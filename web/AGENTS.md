# web/ — 前端域规则(叠加根 AGENTS.md)

## 结构与依赖

```
apps/main        组合根:路由组装、布局、登录、全局 Provider、命令面板
packages/*       design-tokens / ui / api-client / events / shared / ai-ui / icons
features/*       业务域包,与后端域一一对应(workitem/repo/pipeline/deploy/insights/search/admin)
tooling/*        共享 eslint/tsconfig/prettier
```

- `features/A` 禁止 import `features/B`;跨域跳转只走路由;需要他域数据 → 后端提供聚合端点,不要前端拼装;
- `packages/*` 禁止 import `features/*`;
- `packages/api-client`、`packages/events` 是生成物:**禁止手改**,契约变更走后端 utoipa → 重新生成。

## 公共代码放置决策树(前端版)

```
只在当前 feature 用 → feature 内部 utils/(到此为止)
2 个以上 feature 用:
  ├─ 纯 hooks/工具(useDebounce/日期格式化)→ packages/shared
  ├─ 视觉组件(Button/Modal 变体/状态徽标)→ packages/ui + Storybook 文档
  └─ 样式值(颜色/间距/圆角/阴影)→ packages/design-tokens
AI 助手相关(流式消息/生成式业务卡片)→ packages/ai-ui
```

判定示例:
- ✅ 看板和列表页都要"工作项状态徽标" → `packages/ui/StatusBadge`
- ❌ 在 feature 里直接写 `color: '#3b5bdb'` → 一切取值经 design-tokens
- ❌ 两个 feature 各自复制同一张卡片组件 → 提升到 `packages/ui`

## UI 规范

- 组件基座 shadcn/ui(Radix + Tailwind);新建组件前先查 `packages/ui` 与 Storybook——复用优先;
- 样式值只从 design-tokens 出(色/字/距/角/影);暗色模式必须同时验证;
- 密度默认紧凑;代码/ID/日志一律等宽字体;
- 表格用 TanStack Table 封装(大列表必须开虚拟化);拖拽用 dnd-kit;图表用 ECharts 并注册全局主题;
- 文案 zh-CN;空状态有插画与下一步引导;加载有骨架屏。

## AI 原生 UI

- 聊天/流式/生成式卡片一律经 `packages/ai-ui`(Vercel AI SDK + assistant-ui);
- 生成式卡片的 payload schema 用 zod 定义并与后端 MCP 工具输出对齐;
- 所有 AI 功能入口必须可被平台总开关关闭,关闭时页面无残留入口。

## 质量门槛

- tsc strict、禁 any;`pnpm lint + test` 通过;
- 新组件进 Storybook;改 tokens 要出前后对比截图;
- 接口错误统一走 shared 的错误处理(映射后端错误码 `<域>-<3位>`)。
