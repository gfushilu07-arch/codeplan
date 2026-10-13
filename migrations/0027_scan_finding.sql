-- 安全扫描发现(T-305):Semgrep/gitleaks/trivy 结果模型
CREATE TABLE IF NOT EXISTS ci_scan_finding (
    id         UUID PRIMARY KEY,
    repo_id    UUID         NOT NULL,
    commit_sha VARCHAR(64)  NOT NULL,
    tool       VARCHAR(32)  NOT NULL,
    severity   VARCHAR(16)  NOT NULL,
    rule_id    VARCHAR(255) NOT NULL DEFAULT '',
    file_path  TEXT         NOT NULL DEFAULT '',
    line       INTEGER,
    message    TEXT         NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ  NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_ci_scan_finding_sha
    ON ci_scan_finding (repo_id, commit_sha);
