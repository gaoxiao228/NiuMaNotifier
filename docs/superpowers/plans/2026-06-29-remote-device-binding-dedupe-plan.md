# Remote Device Binding Dedupe Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix repeated remote device binding so the same local device maps to one active server record, comes online quickly after binding, and existing duplicate records can be manually cleaned.

**Architecture:** Persist a stable local install ID and derive `device_fingerprint` from `server_url + install_id`; enforce idempotency on the remote server with a partial unique index and atomic upsert; wake the local remote agent after binding instead of waiting for its polling sleep; provide a dry-run-first cleanup script for historical duplicate device names.

**Tech Stack:** Rust/Tauri, `niuma-core` store helpers, Node.js/TypeScript remote server, Drizzle/PostgreSQL, Vitest, Cargo tests.

---

## File Structure

- Modify `crates/niuma-core/src/remote/device_identity.rs`: add hex encode/decode for `DeviceInstallId`.
- Modify `crates/niuma-core/src/store/config_files.rs`: read/write `remote-device-install-id.json`.
- Modify `crates/niuma-core/src/store.rs`: expose `remote_device_install_id()`.
- Modify `crates/niuma-core/src/store/tests.rs`: cover persistent install ID behavior.
- Modify `src-tauri/src/remote/login_flow.rs`: use persisted install ID instead of generating a fresh one.
- Modify `src-tauri/src/remote/agent.rs`: add wake notification and interruptible sleep.
- Modify `src-tauri/src/main.rs`, `src-tauri/src/background.rs`, `src-tauri/src/commands.rs`: pass the wake handle and notify it after binding.
- Modify `remote-server/src/modules/desktopLogin/desktopLogin.repository.ts`: make device upsert atomic.
- Modify `remote-server/src/modules/desktopLogin/desktopLogin.service.ts`: remove obsolete pre-query dependency from repository contract.
- Modify `remote-server/tests/desktopLogin.service.test.ts`: assert repeated same fingerprint updates one device.
- Add `remote-server/migrations/0001_devices_active_fingerprint_unique.sql`: partial unique index.
- Add `remote-server/scripts/dedupe-devices.ts`: dry-run/apply cleanup script.
- Modify `remote-server/package.json`: add `devices:dedupe` script.
- Add `remote-server/tests/devices-dedupe-script.test.ts`: test dry-run and apply grouping.

---

### Task 1: Persist Stable Local Device Install ID

**Files:**
- Modify: `crates/niuma-core/src/remote/device_identity.rs`
- Modify: `crates/niuma-core/src/store/config_files.rs`
- Modify: `crates/niuma-core/src/store.rs`
- Modify: `crates/niuma-core/src/store/tests.rs`
- Modify: `src-tauri/src/remote/login_flow.rs`

- [ ] **Step 1: Add failing tests for install ID hex parsing**

Add to `crates/niuma-core/src/remote/device_identity.rs` tests:

```rust
#[test]
fn device_install_id_round_trips_hex() {
    let install_id = DeviceInstallId::from_bytes([9u8; 32]);
    let encoded = install_id.to_hex();

    assert_eq!(encoded.len(), 64);
    assert_eq!(
        DeviceInstallId::from_hex(&encoded).unwrap().as_bytes(),
        install_id.as_bytes()
    );
}

#[test]
fn device_install_id_rejects_invalid_hex() {
    assert!(DeviceInstallId::from_hex("abc").is_err());
    assert!(DeviceInstallId::from_hex(&"z".repeat(64)).is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p niuma-core remote::device_identity
```

Expected: FAIL because `to_hex` and `from_hex` do not exist.

- [ ] **Step 3: Implement hex helpers**

Update `DeviceInstallId` in `crates/niuma-core/src/remote/device_identity.rs`:

```rust
impl DeviceInstallId {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        Self(bytes)
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn from_hex(value: &str) -> Result<Self, String> {
        if value.len() != 64 {
            return Err("远程设备安装 ID 长度无效".to_string());
        }
        let decoded = hex::decode(value)
            .map_err(|error| format!("远程设备安装 ID 不是有效 hex：{error}"))?;
        let bytes: [u8; 32] = decoded
            .try_into()
            .map_err(|_| "远程设备安装 ID 字节长度无效".to_string())?;
        Ok(Self(bytes))
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}
```

- [ ] **Step 4: Add failing store tests for persistent install ID**

Add to `crates/niuma-core/src/store/tests.rs`:

```rust
#[test]
fn remote_device_install_id_is_created_once_and_reused() {
    let root = test_data_dir("remote_device_install_id_reuse");
    let path = root.join("state.sqlite");
    let store = NiumaStore::new(path.clone());

    let first = store.remote_device_install_id().unwrap();
    let second = NiumaStore::new(path).remote_device_install_id().unwrap();

    assert_eq!(first, second);
    assert_eq!(first.to_hex().len(), 64);
}
```

- [ ] **Step 5: Run test to verify it fails**

Run:

```bash
cargo test -p niuma-core remote_device_install_id_is_created_once_and_reused
```

Expected: FAIL because `remote_device_install_id` does not exist.

- [ ] **Step 6: Implement config-file persistence**

In `crates/niuma-core/src/store/config_files.rs`, add imports:

```rust
use crate::remote::device_identity::DeviceInstallId;
```

Add struct near `AppConfigFile`:

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
struct RemoteDeviceInstallIdFile {
    version: u32,
    install_id: String,
}
```

Add methods inside `impl ConfigFileStore`:

```rust
    pub(super) fn remote_device_install_id(&self) -> Result<DeviceInstallId, String> {
        let path = self.remote_device_install_id_path();
        if path.exists() {
            let value: RemoteDeviceInstallIdFile = serde_json::from_value(read_json_file(&path)?)
                .map_err(|error| format!("解析远程设备安装 ID 失败：{error}"))?;
            if value.version != 1 {
                return Err(format!("不支持的远程设备安装 ID 版本：{}", value.version));
            }
            return DeviceInstallId::from_hex(&value.install_id);
        }

        let install_id = DeviceInstallId::generate();
        let value = serde_json::to_value(RemoteDeviceInstallIdFile {
            version: 1,
            install_id: install_id.to_hex(),
        })
        .map_err(|error| format!("序列化远程设备安装 ID 失败：{error}"))?;
        write_json_file(&path, &value)?;
        Ok(install_id)
    }

    fn remote_device_install_id_path(&self) -> PathBuf {
        self.root.join("remote-device-install-id.json")
    }
```

In `crates/niuma-core/src/store.rs`, expose:

```rust
    pub fn remote_device_install_id(
        &self,
    ) -> Result<crate::remote::device_identity::DeviceInstallId, String> {
        self.config_files().remote_device_install_id()
    }
```

- [ ] **Step 7: Update login flow to accept store-backed install ID**

Change `src-tauri/src/remote/login_flow.rs` function signature:

```rust
pub async fn start_remote_login_session(
    config: &RemoteConfig,
    install_id: DeviceInstallId,
) -> Result<RemoteLoginStarted, String> {
    let device_fingerprint = derive_device_fingerprint(&config.server_url, &install_id);
    // existing key generation and request code stays the same.
}
```

Update `src-tauri/src/commands.rs` in `start_remote_login` before calling the flow:

```rust
    let install_id = match runtime_state.store.remote_device_install_id() {
        Ok(value) => value,
        Err(error) => return Ok(ApiResponse::fail(ApiErrorCode::System, error)),
    };
```

Then call:

```rust
crate::remote::login_flow::start_remote_login_session(&config, install_id).await
```

- [ ] **Step 8: Update Rust tests that call the old signature**

Search:

```bash
rg -n "start_remote_login_session\\(" src-tauri crates
```

Update any direct call to pass `DeviceInstallId::from_bytes([7u8; 32])`.

- [ ] **Step 9: Run focused Rust tests**

Run:

```bash
cargo test -p niuma-core remote::device_identity remote_device_install_id_is_created_once_and_reused
cargo test -p niuma-desktop remote::login_flow
```

Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add crates/niuma-core/src/remote/device_identity.rs crates/niuma-core/src/store/config_files.rs crates/niuma-core/src/store.rs crates/niuma-core/src/store/tests.rs src-tauri/src/remote/login_flow.rs src-tauri/src/commands.rs
git commit -m "fix: 持久化远程设备安装标识" -m "修改内容：新增本机远程设备安装 ID 的持久化读写，并用稳定安装 ID 生成设备指纹。" -m "修改原因：避免同一设备每次登录绑定生成不同指纹，导致服务端插入重复设备。"
```

---

### Task 2: Enforce Server Device Upsert Idempotency

**Files:**
- Modify: `remote-server/src/modules/desktopLogin/desktopLogin.service.ts`
- Modify: `remote-server/src/modules/desktopLogin/desktopLogin.repository.ts`
- Modify: `remote-server/tests/desktopLogin.service.test.ts`
- Add: `remote-server/migrations/0001_devices_active_fingerprint_unique.sql`

- [ ] **Step 1: Add failing service test for repeated same fingerprint**

Add to `remote-server/tests/desktopLogin.service.test.ts`:

```ts
it('reuses active device for the same user and fingerprint', async () => {
  const first = await validStartInput()
  const second = await validStartInput()
  second.input.device_fingerprint = first.input.device_fingerprint
  const repo = createRepo()
  const service = createDesktopLoginService({
    repo,
    config: {
      publicUrl: 'https://remote.example.com',
      tokenPepper: 'pepper',
      desktopLoginTtlSeconds: 600
    }
  })

  const startOne = await service.start(first.input)
  const startTwo = await service.start(second.input)
  if (!startOne.ok || !startTwo.ok) throw new Error('start failed')

  await service.complete({
    requestId: startOne.data.request_id,
    user: { id: 'usr_1', email: 'user@example.com', role: 'user' }
  })
  await service.complete({
    requestId: startTwo.data.request_id,
    user: { id: 'usr_1', email: 'user@example.com', role: 'user' }
  })

  const completedOne = await service.poll({
    request_id: startOne.data.request_id,
    poll_token: startOne.data.poll_token
  })
  const completedTwo = await service.poll({
    request_id: startTwo.data.request_id,
    poll_token: startTwo.data.poll_token
  })
  expect(completedOne.ok).toBe(true)
  expect(completedTwo.ok).toBe(true)
  if (!completedOne.ok || !completedTwo.ok) throw new Error('poll failed')

  const deviceOne = JSON.parse(
    new TextDecoder().decode((await compactDecrypt(completedOne.data.encrypted_result.jwe, first.privateKey)).plaintext)
  ).device
  const deviceTwo = JSON.parse(
    new TextDecoder().decode((await compactDecrypt(completedTwo.data.encrypted_result.jwe, second.privateKey)).plaintext)
  ).device

  expect(deviceTwo.id).toBe(deviceOne.id)
})
```

- [ ] **Step 2: Run focused test**

Run:

```bash
cd remote-server
npm test -- desktopLogin.service.test.ts
```

Expected: PASS with in-memory repo if it already models upsert, but this test locks the desired contract before repository changes.

- [ ] **Step 3: Simplify repository contract**

In `remote-server/src/modules/desktopLogin/desktopLogin.service.ts`, remove `findActiveDeviceByFingerprint` from `DesktopLoginRepository`. `complete()` should only call `repo.upsertDevice(...)`.

Update `createRepo()` in `remote-server/tests/desktopLogin.service.test.ts` by removing `findActiveDeviceByFingerprint`.

- [ ] **Step 4: Add migration**

Create `remote-server/migrations/0001_devices_active_fingerprint_unique.sql`:

```sql
CREATE UNIQUE INDEX IF NOT EXISTS "devices_active_user_fingerprint_unique"
ON "devices" ("user_id", "fingerprint_hash")
WHERE "status" = 'active';
```

- [ ] **Step 5: Implement atomic upsert**

Replace `upsertDevice` in `remote-server/src/modules/desktopLogin/desktopLogin.repository.ts` with parameterized SQL:

```ts
    async upsertDevice(input) {
      const row = (
        await db.execute(sql`
          INSERT INTO devices (
            user_id,
            name,
            fingerprint_hash,
            token_hash,
            identity_public_key_json,
            status,
            capability_json,
            created_at,
            updated_at,
            revoked_at
          )
          VALUES (
            ${input.userId},
            ${input.name},
            ${input.fingerprintHash},
            ${input.tokenHash},
            ${JSON.stringify(input.identityPublicKeyJson)}::jsonb,
            ${input.status},
            ${JSON.stringify(input.capabilityJson)}::jsonb,
            ${input.createdAt},
            ${input.updatedAt},
            ${input.revokedAt}
          )
          ON CONFLICT (user_id, fingerprint_hash)
          WHERE status = 'active'
          DO UPDATE SET
            name = EXCLUDED.name,
            token_hash = EXCLUDED.token_hash,
            identity_public_key_json = EXCLUDED.identity_public_key_json,
            capability_json = EXCLUDED.capability_json,
            updated_at = EXCLUDED.updated_at,
            revoked_at = NULL
          RETURNING id, name
        `)
      ).rows[0]
      return row as { id: string; name: string }
    }
```

Also update import:

```ts
import { eq, isNull, sql } from 'drizzle-orm'
```

Remove unused `and` only if no longer needed in this file.

- [ ] **Step 6: Run remote-server tests**

Run:

```bash
cd remote-server
npm test -- desktopLogin.service.test.ts
npm run build
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add remote-server/src/modules/desktopLogin/desktopLogin.service.ts remote-server/src/modules/desktopLogin/desktopLogin.repository.ts remote-server/tests/desktopLogin.service.test.ts remote-server/migrations/0001_devices_active_fingerprint_unique.sql
git commit -m "fix: 保证远程设备绑定幂等" -m "修改内容：新增 active 设备唯一索引，并将设备绑定改为基于用户和指纹的原子 upsert。" -m "修改原因：避免重复登录或并发绑定时产生多条 active 设备记录。"
```

---

### Task 3: Add Manual Historical Device Dedupe Script

**Files:**
- Add: `remote-server/scripts/dedupe-devices.ts`
- Add: `remote-server/tests/devices-dedupe-script.test.ts`
- Modify: `remote-server/package.json`

- [ ] **Step 1: Add script tests around pure planning logic**

Create `remote-server/tests/devices-dedupe-script.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { planDeviceDedupe } from '../scripts/dedupe-devices.js'

describe('device dedupe script planning', () => {
  it('keeps latest active device per name', () => {
    const plan = planDeviceDedupe([
      { id: 'dev_old', name: 'NiuMa Device', created_at: '2026-06-28T00:00:00.000Z' },
      { id: 'dev_new', name: 'NiuMa Device', created_at: '2026-06-29T00:00:00.000Z' },
      { id: 'dev_other', name: 'Other Device', created_at: '2026-06-27T00:00:00.000Z' }
    ])

    expect(plan).toEqual([
      {
        name: 'NiuMa Device',
        keep: { id: 'dev_new', name: 'NiuMa Device', created_at: '2026-06-29T00:00:00.000Z' },
        revoke: [{ id: 'dev_old', name: 'NiuMa Device', created_at: '2026-06-28T00:00:00.000Z' }]
      }
    ])
  })

  it('does not revoke unique names', () => {
    expect(
      planDeviceDedupe([
        { id: 'dev_1', name: 'One', created_at: '2026-06-28T00:00:00.000Z' },
        { id: 'dev_2', name: 'Two', created_at: '2026-06-28T00:00:00.000Z' }
      ])
    ).toEqual([])
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- devices-dedupe-script.test.ts
```

Expected: FAIL because `scripts/dedupe-devices.ts` does not exist.

- [ ] **Step 3: Implement cleanup script**

Create `remote-server/scripts/dedupe-devices.ts`:

```ts
import pg from 'pg'
import { loadConfig } from '../src/config.js'

export type DeviceDedupeRow = {
  id: string
  name: string
  created_at: string
}

export type DeviceDedupeGroup = {
  name: string
  keep: DeviceDedupeRow
  revoke: DeviceDedupeRow[]
}

export function planDeviceDedupe(rows: DeviceDedupeRow[]): DeviceDedupeGroup[] {
  const groups = new Map<string, DeviceDedupeRow[]>()
  for (const row of rows) {
    groups.set(row.name, [...(groups.get(row.name) ?? []), row])
  }

  return [...groups.entries()]
    .map(([name, devices]) => {
      const sorted = [...devices].sort((a, b) => b.created_at.localeCompare(a.created_at))
      return { name, keep: sorted[0], revoke: sorted.slice(1) }
    })
    .filter((group) => group.revoke.length > 0)
}

function readArg(name: string) {
  const index = process.argv.indexOf(name)
  return index >= 0 ? process.argv[index + 1] : undefined
}

async function main() {
  const userEmail = readArg('--user-email')
  const apply = process.argv.includes('--apply')
  if (!userEmail) throw new Error('缺少 --user-email')

  const config = loadConfig()
  const client = new pg.Client({ connectionString: config.databaseUrl })
  await client.connect()
  try {
    const user = (await client.query('select id, email from users where email = $1 limit 1', [userEmail])).rows[0]
    if (!user) throw new Error(`用户不存在：${userEmail}`)

    const devices = (
      await client.query(
        'select id, name, created_at from devices where user_id = $1 and status = $2 order by name, created_at desc',
        [user.id, 'active']
      )
    ).rows as DeviceDedupeRow[]
    const plan = planDeviceDedupe(devices)

    console.log(JSON.stringify({ user, apply, groups: plan }, null, 2))
    if (!apply || plan.length === 0) return

    const ids = plan.flatMap((group) => group.revoke.map((device) => device.id))
    await client.query(
      'update devices set status = $1, revoked_at = now(), updated_at = now() where user_id = $2 and id = any($3::uuid[])',
      ['revoked', user.id, ids]
    )
  } finally {
    await client.end()
  }
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.message : error)
    process.exitCode = 1
  })
}
```

- [ ] **Step 4: Add npm script**

Modify `remote-server/package.json`:

```json
"devices:dedupe": "tsx scripts/dedupe-devices.ts"
```

- [ ] **Step 5: Run tests and build**

Run:

```bash
cd remote-server
npm test -- devices-dedupe-script.test.ts
npm run build
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add remote-server/scripts/dedupe-devices.ts remote-server/tests/devices-dedupe-script.test.ts remote-server/package.json
git commit -m "feat: 新增远程设备重复记录清理脚本" -m "修改内容：新增按用户和设备名规划重复设备清理的脚本，支持 dry-run 和 apply。" -m "修改原因：已有历史重复设备需要手动确认后清理，避免自动合并误伤。"
```

---

### Task 4: Wake Remote Agent After Binding

**Files:**
- Modify: `src-tauri/src/remote/agent.rs`
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/src/background.rs`
- Modify: `src-tauri/src/commands.rs`

- [ ] **Step 1: Add agent wake unit test**

Add to `src-tauri/src/remote/agent.rs` tests:

```rust
#[test]
fn remote_agent_wake_signal_can_be_requested() {
    let wake = RemoteAgentWake::default();

    assert!(!wake.take_requested());
    wake.request();
    assert!(wake.take_requested());
    assert!(!wake.take_requested());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p niuma-desktop remote::agent::remote_agent_wake_signal_can_be_requested
```

Expected: FAIL because `RemoteAgentWake` does not exist.

- [ ] **Step 3: Implement wake handle**

Add to `src-tauri/src/remote/agent.rs`:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
```

Add type:

```rust
#[derive(Clone, Default)]
pub struct RemoteAgentWake {
    requested: Arc<AtomicBool>,
}

impl RemoteAgentWake {
    pub fn request(&self) {
        self.requested.store(true, Ordering::SeqCst);
    }

    pub fn take_requested(&self) -> bool {
        self.requested.swap(false, Ordering::SeqCst)
    }
}
```

Add helper:

```rust
async fn sleep_or_wake(wake: &RemoteAgentWake, duration: Duration) {
    let mut elapsed = Duration::ZERO;
    while elapsed < duration {
        if wake.take_requested() {
            return;
        }
        let step = Duration::from_millis(250);
        time::sleep(step).await;
        elapsed += step;
    }
}
```

Change `run_agent_loop` signature:

```rust
pub async fn run_agent_loop(
    mut load_config: impl FnMut() -> Result<RemoteConfig, String>,
    credential_store: impl RemoteCredentialStore,
    status: RemoteAgentStatusHandle,
    wake: RemoteAgentWake,
) {
```

Replace each `time::sleep(Duration::from_secs(...)).await` inside the loop with `sleep_or_wake(&wake, Duration::from_secs(...)).await`.

Change `spawn_remote_agent_runtime` signature:

```rust
pub fn spawn_remote_agent_runtime(
    store: NiumaStore,
    status: RemoteAgentStatusHandle,
    wake: RemoteAgentWake,
) {
```

Pass `wake` into `run_agent_loop`.

- [ ] **Step 4: Wire wake through runtime state**

In `src-tauri/src/main.rs`, create:

```rust
let remote_agent_wake = remote::agent::RemoteAgentWake::default();
```

Add to `AppRuntimeState` construction:

```rust
remote_agent_wake: remote_agent_wake.clone(),
```

Pass to background startup:

```rust
remote_agent_wake.clone(),
```

In `src-tauri/src/commands.rs`, add field:

```rust
pub(crate) remote_agent_wake: crate::remote::agent::RemoteAgentWake,
```

After successful `apply_remote_binding_result`, request wake:

```rust
let response = crate::remote::commands::apply_remote_binding_result(
    &runtime_state.store,
    &credential_store,
    &config.server_url,
    poll_result.device_identity_private_key,
    binding,
);
if response.code == 0 {
    runtime_state.remote_agent_wake.request();
}
return Ok(response);
```

In `src-tauri/src/background.rs`, add parameter:

```rust
remote_agent_wake: remote::agent::RemoteAgentWake,
```

Pass it:

```rust
remote::agent::spawn_remote_agent_runtime(
    store.clone(),
    remote_agent_status.clone(),
    remote_agent_wake.clone(),
);
```

- [ ] **Step 5: Run focused Rust tests**

Run:

```bash
cargo test -p niuma-desktop remote::agent
cargo check -p niuma-desktop
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/remote/agent.rs src-tauri/src/main.rs src-tauri/src/background.rs src-tauri/src/commands.rs
git commit -m "fix: 绑定后立即唤醒远程 agent" -m "修改内容：新增远程 agent 唤醒信号，并在登录绑定成功后触发重新连接。" -m "修改原因：避免绑定成功后等待轮询睡眠导致控制台长时间显示离线。"
```

---

### Task 5: Verify Docker Flow And Clean Existing Duplicates

**Files:**
- No source edits expected unless verification reveals a defect.

- [ ] **Step 1: Run full focused verification**

Run:

```bash
cargo test -p niuma-core remote
cargo test -p niuma-desktop remote
cd remote-server && npm test && npm run build
```

Expected: all PASS.

- [ ] **Step 2: Rebuild Docker remote server**

Run:

```bash
cd remote-server
docker compose up -d --build remote-server
docker compose ps
```

Expected: `remote-server-remote-server-1` is running and maps `0.0.0.0:27880->27880/tcp`.

- [ ] **Step 3: Apply migration**

Run:

```bash
cd remote-server
docker compose exec -T remote-server npm run db:migrate
```

Expected: migration completes without duplicate-index error.

- [ ] **Step 4: Dry-run duplicate cleanup**

Run with the actual test account email:

```bash
cd remote-server
docker compose exec -T remote-server npm run devices:dedupe -- --user-email user@example.com --keep latest --dry-run
```

Expected: JSON output lists duplicate `NiuMa Device` rows and shows `apply: false`.

- [ ] **Step 5: Apply duplicate cleanup after user confirmation**

Run only after dry-run output is reviewed:

```bash
cd remote-server
docker compose exec -T remote-server npm run devices:dedupe -- --user-email user@example.com --keep latest --apply
```

Expected: old duplicate devices are marked `revoked`; only the latest same-name active device remains.

- [ ] **Step 6: Manual UI verification**

1. Open the local NiuMaNotifier settings page.
2. Run “登录并绑定” twice against `http://127.0.0.1:27880`.
3. Open `http://127.0.0.1:27880/`.
4. Login to the Web console.
5. Confirm the device table shows one `NiuMa Device`.
6. Refresh after a few seconds and confirm status becomes online.

- [ ] **Step 7: Final commit only if verification caused docs/script adjustment**

If verification required a small follow-up edit, add the exact files changed by that verification edit:

```bash
git add src-tauri/src/remote/agent.rs remote-server/scripts/dedupe-devices.ts
git commit -m "fix: 完善远程设备绑定去重验证问题" -m "修改内容：根据 Docker 和手动验收结果修正远程设备去重实现细节。" -m "修改原因：确保重复绑定、上线状态和历史清理流程完整可用。"
```
