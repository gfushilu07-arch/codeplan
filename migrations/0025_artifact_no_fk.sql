-- T-304:registry 级制品(Maven/npm/PyPI/OCI 公共空间)repo_id 用 nil UUID,
-- 无对应行 — 移除 FK(仓库级制品仍以真实 repo_id 记录)。
ALTER TABLE reg_artifact DROP CONSTRAINT IF EXISTS reg_artifact_repo_id_fkey;
