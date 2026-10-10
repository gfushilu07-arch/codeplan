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

/** 看板页(T-132):按状态分列 + 拖拽跨列(调 reorder API,非法流转由后端 409)。 */
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
    // 乐观更新:先移列,失败回滚重新拉取
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

  return (
    <div>
      <div style={{ display: "flex", alignItems: "center", gap: "var(--cp-space-2)", marginBottom: "var(--cp-space-3)" }}>
        <h2 style={{ fontSize: 18, margin: 0 }}>看板</h2>
        <select
          value={activeKey ?? ""}
          onChange={(e) => setActiveKey(e.target.value)}
          style={{
            border: "1px solid var(--cp-color-border)",
            borderRadius: "var(--cp-radius-md)",
            padding: "4px 8px",
            background: "var(--cp-color-bg)",
            color: "var(--cp-color-text)",
            font: "inherit",
            fontSize: 13,
          }}
        >
          {projectOptions}
        </select>
        {busy && <span style={{ fontSize: 12, color: "var(--cp-color-text-muted)" }}>同步中…</span>}
      </div>
      {error && <div style={{ color: "var(--cp-color-danger)", fontSize: 13, marginBottom: 8 }}>{error}</div>}
      <DndContext sensors={sensors} onDragEnd={onDragEnd}>
        <div style={{ display: "flex", gap: "var(--cp-space-3)", alignItems: "flex-start" }}>
          {columns.map((col) => (
            <BoardColumnView key={col.status} column={col} />
          ))}
        </div>
      </DndContext>
    </div>
  );
}

function BoardColumnView({ column }: { column: BoardColumn }) {
  const { setNodeRef } = useDroppable({ id: column.status });
  return (
    <div
      ref={setNodeRef}
      style={{
        flex: 1,
        minWidth: 240,
        background: "var(--cp-color-surface)",
        border: "1px solid var(--cp-color-border)",
        borderRadius: "var(--cp-radius-md)",
        padding: "var(--cp-space-2)",
      }}
    >
      <div
        style={{
          fontSize: 13,
          fontWeight: 600,
          color: "var(--cp-color-text-muted)",
          marginBottom: "var(--cp-space-2)",
          display: "flex",
          justifyContent: "space-between",
        }}
      >
        <span>{column.status}</span>
        <span>{column.total}</span>
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: "var(--cp-space-2)" }}>
        {column.items.map((item) => (
          <BoardCard key={item.id} item={item} />
        ))}
      </div>
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
      style={{
        background: "var(--cp-color-bg)",
        border: "1px solid var(--cp-color-border)",
        borderRadius: "var(--cp-radius-md)",
        padding: "var(--cp-space-2)",
        fontSize: 13,
        cursor: isDragging ? "grabbing" : "grab",
        opacity: isDragging ? 0.5 : 1,
        boxShadow: "var(--cp-shadow-1)",
        color: "var(--cp-color-text)",
      }}
    >
      <div style={{ fontFamily: "var(--cp-font-mono)", fontSize: 11, color: "var(--cp-color-text-muted)" }}>
        #{item.number}
      </div>
      <div>{item.title}</div>
    </div>
  );
}
