-- 事务发件箱(T-110):业务写入与事件同事务落库,dispatcher 转发 NATS
CREATE TABLE IF NOT EXISTS event_outbox (
    id            UUID PRIMARY KEY,
    event_type    VARCHAR(128) NOT NULL,
    payload       JSONB        NOT NULL DEFAULT '{}',
    actor         VARCHAR(64),
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT now(),
    published_at  TIMESTAMPTZ
);
CREATE INDEX IF NOT EXISTS idx_event_outbox_unpublished
    ON event_outbox (created_at) WHERE published_at IS NULL;
