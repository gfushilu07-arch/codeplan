<!-- 标题格式:feat(domain): 简述 (#工作项ID) -->

## 关联工作项

<!-- 必填:关联的功能/任务 ID,如 PAAS-07 / T-316 -->

## 改动说明

<!-- 做了什么、为什么这样做 -->

## 自检清单(DoD)

- [ ] `cargo clippy --workspace -- -D warnings` 通过
- [ ] `cargo test --workspace` 通过
- [ ] `cargo xtask lint-deps` 通过(新增跨 crate 依赖时)
- [ ] 新端点有 utoipa 注解 / 新事件有 kernel::events schema / 迁移向后兼容
- [ ] `pnpm lint && pnpm test` 通过(涉及前端时)
- [ ] 更新了受影响的 AGENTS.md 规则(涉及架构决策时)
