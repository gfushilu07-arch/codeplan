import { useEffect, useMemo, useState } from "react";
import {
  DndContext,
  PointerSensor,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  fetchBoard,
  listProjects,
  reorderBoard,
  type BoardColumn,
  type BoardItem,
  type ProjectSummary,
} from "@codeplan/api-client";
import { statusLabel } from "./status";

/** 看板页(T-132):按状态分列 + 拖拽跨列(非法流转由后端 409)。 */
export function BoardViewPage() {
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [activeKey, setActiveKey] = useState<string | null>(null);
  const [columns, setColumns] = useState<BoardColumn[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const sensors = useSensors(useSensor(PointerSensor));

  useEffect(() => {
    listProjects()
      .then((ps) => {
        setProjects(ps);
        if (ps.length > 0) setActiveKey(ps[0].key);
      })
      .catch((e: Error) => setError(e.message));
  }, []);

  useEffect(() => {
    if (!activeKey) return;
    setError(null);
    fetchBoard(activeKey)
      .then((b) => setColumns(b.columns))
      .catch((e: Error) => setError(e.message));
  }, [activeKey]);

  const onDragEnd = async (event: DragEndEvent) => {
    const id = String(event.active.id);
    const targetStatus = event.over?.id as string | undefined;
    if (!targetStatus || !activeKey || busy) return;
    const from = columns.find((c) => c.items.some((i) => i.id === id));
    if (!from || from.status === targetStatus) return;
    setBusy(true);
    setColumns((cols) =>
      cols.map((c) => {
        if (c.status === from.status) {
          return { ...c, items: c.items.filter((i) => i.id !== id), total: c.total - 1 };
        }
        if (c.status === targetStatus) {
          const item = from.items.find((i) => i.id === id);
          if (item) {
            const moved: BoardItem = { ...item, board_order: c.items.length };
            return { ...c, items: [...c.items, moved], total: c.total + 1 };
          }
        }
        return c;
      }),
    );
    try {
      await reorderBoard(activeKey, [{ id, status: targetStatus, board_order: 0 }]);
    } catch (e) {
      setError((e as Error).message);
      fetchBoard(activeKey).then((b) => setColumns(b.columns));
    } finally {
      setBusy(false);
    }
  };

  const projectOptions = useMemo(
    () =>
      projects.map((p) => (
        <option key={p.id} value={p.key}>
          {p.key} · {p.name}
        </option>
      )),
    [projects],
  );

  const total = columns.reduce((acc, c) => acc + c.total, 0);

  return (
    <div className="cp-page">
      <div className="cp-page-head">
        <div>
          <h1 className="cp-page-title">项目看板</h1>
          <div className="cp-page-sub">拖拽卡片可跨列流转状态</div>
        </div>
        <div className="cp-page-actions">
          <select value={activeKey ?? ""} onChange={(e) => setActiveKey(e.target.value)}>
            {projectOptions}
          </select>
          {busy && <span className="cp-muted" style={{ fontSize: 12 }}>同步中…</span>}
          <span className="cp-badge cp-badge-neutral">共 {total} 项</span>
        </div>
      </div>

      {error && <div className="cp-alert cp-alert-danger">{error}</div>}

      <DndContext sensors={sensors} onDragEnd={onDragEnd}>
        <div className="cp-board">
          {columns.map((col) => (
            <BoardColumnView key={col.status} column={col} />
          ))}
          {columns.length === 0 && (
            <div className="cp-card" style={{ flex: 1 }}>
              <div className="cp-empty">
                <div className="cp-empty-icon">▦</div>
                <div className="cp-empty-title">暂无看板数据</div>
                <div className="cp-empty-hint">请选择包含工作项的项目</div>
              </div>
            </div>
          )}
        </div>
      </DndContext>
    </div>
  );
}

function BoardColumnView({ column }: { column: BoardColumn }) {
  const { setNodeRef } = useDroppable({ id: column.status });
  return (
    <div ref={setNodeRef} className="cp-board-col">
      <div className="cp-board-col-head">
        <span>{statusLabel(column.status)}</span>
        <span className="count">{column.total}</span>
      </div>
      {column.items.map((item) => (
        <BoardCard key={item.id} item={item} />
      ))}
      {column.items.length === 0 && (
        <div className="cp-muted" style={{ fontSize: 12, padding: "12px 6px", textAlign: "center" }}>
          拖拽到此列
        </div>
      )}
    </div>
  );
}

function BoardCard({ item }: { item: BoardItem }) {
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({ id: item.id });
  return (
    <div
      ref={setNodeRef}
      {...attributes}
      {...listeners}
      className={`cp-board-card${isDragging ? " dragging" : ""}`}
    >
      <div className="key">#{item.number}</div>
      <div className="title">{item.title}</div>
      {/* 阻止 pointerdown 冒泡,避免被 dnd-kit 当作拖拽起点而吞掉点击 */}
      <a
        href={`#/item/${item.id}`}
        className="cp-muted"
        style={{ fontSize: 12, alignSelf: "flex-start" }}
        onPointerDown={(e) => e.stopPropagation()}
      >
        详情 →
      </a>
    </div>
  );
}
