#!/bin/sh
# CodePlan 备份脚本(ADMIN-02):PG 逻辑备份 + 裸仓库 + 附件/Wiki
# 用法:CODEPLAN_DATA_DIR=./data BACKUP_DIR=./backups ./deploy/backup.sh
set -e

DATA_DIR="${CODEPLAN_DATA_DIR:-./data}"
BACKUP_DIR="${BACKUP_DIR:-./backups}"
STAMP=$(date +%Y%m%d-%H%M%S)
TARGET="$BACKUP_DIR/$STAMP"

mkdir -p "$TARGET"

echo "[1/3] PostgreSQL 逻辑备份…"
docker exec codeplan-dev-postgres-1 pg_dump -U codeplan codeplan > "$TARGET/db.sql"

echo "[2/3] 裸仓库目录…"
tar -czf "$TARGET/repositories.tar.gz" -C "$DATA_DIR" repositories 2>/dev/null || echo "  (无仓库目录,跳过)"

echo "[3/3] 附件/Wiki 文件…"
tar -czf "$TARGET/attachments.tar.gz" -C "$DATA_DIR" attachments 2>/dev/null || echo "  (无附件目录,跳过)"

echo "备份完成:$TARGET"
echo "恢复:pg_restore/psql 导入 db.sql + 解压两个 tar 到原路径"
