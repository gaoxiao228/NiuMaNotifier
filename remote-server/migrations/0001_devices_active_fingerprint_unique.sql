-- 保证同一用户同一设备指纹只有一个 active 设备，配合绑定 upsert 消除并发插入竞态。
CREATE UNIQUE INDEX IF NOT EXISTS "devices_active_user_fingerprint_unique"
ON "devices" ("user_id", "fingerprint_hash")
WHERE "status" = 'active';
