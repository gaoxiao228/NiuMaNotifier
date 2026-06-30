-- remote_connections.id 使用 conn_ 前缀公开 ID，不能使用 PostgreSQL uuid 类型。
ALTER TABLE "remote_connections"
ALTER COLUMN "id" DROP DEFAULT,
ALTER COLUMN "id" TYPE text USING "id"::text;
