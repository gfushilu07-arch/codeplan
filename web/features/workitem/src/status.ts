/** 工作项状态的展示映射(workitem 包内共用:看板 / 需求池 / 工作台)。 */

const STATUS_LABEL: Record<string, string> = {
  todo: "待办",
  open: "待办",
  in_progress: "进行中",
  doing: "进行中",
  in_review: "评审中",
  review: "评审中",
  done: "已完成",
  closed: "已关闭",
  resolved: "已解决",
  blocked: "阻塞",
};

const DONE = new Set(["done", "closed", "resolved"]);

/** 中文标签;未知状态回退原值。 */
export function statusLabel(status: string): string {
  return STATUS_LABEL[status] ?? status;
}

/** 徽章类名(与 global.css 的 .cp-badge-* 对应)。 */
export function statusBadgeClass(status: string): string {
  if (DONE.has(status)) return "cp-badge cp-badge-success";
  if (status === "in_progress" || status === "doing" || status === "in_review" || status === "review") {
    return "cp-badge cp-badge-info";
  }
  if (status === "blocked") return "cp-badge cp-badge-danger";
  return "cp-badge cp-badge-neutral";
}

/** 甘特图/图表用的状态色。 */
export function statusColor(status: string): string {
  if (DONE.has(status)) return "var(--cp-color-success)";
  if (status === "todo" || status === "open") return "var(--cp-color-text-muted)";
  if (status === "blocked") return "var(--cp-color-danger)";
  return "var(--cp-color-primary)";
}
