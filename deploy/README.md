# deploy/ 本地与部署资产

- `docker-compose.yml`:本地开发依赖(PG 16 / Redis 7 / NATS JetStream;MinIO 为可选 profile)
- `backup.sh`:备份脚本(ADMIN-02,随批次一任务实现)

```bash
# 启动核心依赖
docker compose -f docker-compose.yml up -d
# 需要本地 S3(MinIO)时
docker compose -f docker-compose.yml --profile s3 up -d
# 查看健康状态
docker compose -f docker-compose.yml ps
# 停止(保留数据卷)
docker compose -f docker-compose.yml down
```

## 端口说明

本机可能已有其他项目的容器占用了常见端口,故使用如下映射(可在 compose 中改回):

| 服务 | 容器端口 | 本机端口 |
|---|---|---|
| Postgres | 5432 | **5434** |
| Redis | 6379 | **6381** |
| NATS 客户端 | 4222 | **4223** |
| NATS 监控 | 8222 | **8223** |
| MinIO S3 / 控制台 | 9000 / 9001 | 9000 / 9001 |

## 本地默认连接串

- Postgres:`postgres://codeplan:codeplan-dev@localhost:5434/codeplan`
- Redis:`redis://localhost:6381`
- NATS:`nats://localhost:4223`
- 存储后端:开发默认 `CODEPLAN_STORAGE_BACKEND=fs`(文件系统,经 OpenDAL);
  需要 S3 时启用 `--profile s3` 并创建 bucket,`CODEPLAN_STORAGE_BACKEND=s3`。

> MinIO 官方已从 Docker Hub 撤下镜像。若 ghcr.io 不可达,用环境变量覆盖:
> `MINIO_IMAGE=<可用的镜像源地址> docker compose --profile s3 up -d`。
> 密钥仅限本地开发;生产环境密钥经环境注入,不入库(PLAN.md 安全规约)。
