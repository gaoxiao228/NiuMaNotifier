import { fileURLToPath } from 'node:url'
import { and, eq } from 'drizzle-orm'
import { createDb } from '../src/db/client.js'
import { devices, users } from '../src/db/schema.js'

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

type CliOptions = {
  userEmail: string
  keep: 'latest'
  apply: boolean
}

function compareRowsForKeep(left: DeviceDedupeRow, right: DeviceDedupeRow) {
  const timeDiff = Date.parse(right.created_at) - Date.parse(left.created_at)
  if (timeDiff !== 0) return timeDiff
  return left.id.localeCompare(right.id)
}

export function planDeviceDedupe(rows: DeviceDedupeRow[]): DeviceDedupeGroup[] {
  const rowsByName = new Map<string, DeviceDedupeRow[]>()

  for (const row of rows) {
    const group = rowsByName.get(row.name) ?? []
    group.push(row)
    rowsByName.set(row.name, group)
  }

  return [...rowsByName.entries()]
    .filter(([, group]) => group.length > 1)
    .map(([name, group]) => {
      // 排序后第一条是需要保留的记录：created_at 最新；时间相同时 id 升序保证稳定。
      const sorted = [...group].sort(compareRowsForKeep)
      return {
        name,
        keep: sorted[0],
        revoke: sorted.slice(1)
      }
    })
    .sort((left, right) => left.name.localeCompare(right.name))
}

function parseArgs(argv: string[]): CliOptions {
  let userEmail: string | undefined
  let keep = 'latest'
  let apply = false
  let dryRun = false

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    if (arg === '--user-email') {
      userEmail = argv[index + 1]
      index += 1
    } else if (arg === '--keep') {
      keep = argv[index + 1] ?? ''
      index += 1
    } else if (arg === '--apply') {
      apply = true
    } else if (arg === '--dry-run') {
      dryRun = true
    } else {
      throw new Error(`不支持的参数：${arg}`)
    }
  }

  if (!userEmail) {
    throw new Error('必须传入 --user-email')
  }
  if (keep !== 'latest') {
    throw new Error('--keep 目前只支持 latest')
  }
  if (apply && dryRun) {
    throw new Error('--apply 和 --dry-run 不能同时使用')
  }

  return { userEmail, keep, apply }
}

async function runCli(argv: string[]) {
  const options = parseArgs(argv)
  const databaseUrl = process.env.DATABASE_URL
  if (!databaseUrl) {
    throw new Error('必须配置 DATABASE_URL')
  }

  const { db, pool } = createDb(databaseUrl)
  try {
    const user = (
      await db
        .select({ id: users.id, email: users.email })
        .from(users)
        .where(eq(users.email, options.userEmail))
        .limit(1)
    )[0]
    if (!user) {
      throw new Error(`未找到用户：${options.userEmail}`)
    }

    const activeDevices = await db
      .select({
        id: devices.id,
        name: devices.name,
        createdAt: devices.createdAt
      })
      .from(devices)
      .where(and(eq(devices.userId, user.id), eq(devices.status, 'active')))

    const groups = planDeviceDedupe(
      activeDevices.map((device) => ({
        id: device.id,
        name: device.name,
        created_at: device.createdAt.toISOString()
      }))
    )

    if (options.apply) {
      const revokedAt = new Date()
      await db.transaction(async (tx) => {
        for (const group of groups) {
          for (const device of group.revoke) {
            // 只标记当前用户仍处于 active 的重复设备，不做物理删除。
            await tx
              .update(devices)
              .set({ status: 'revoked', revokedAt, updatedAt: revokedAt })
              .where(
                and(
                  eq(devices.userId, user.id),
                  eq(devices.id, device.id),
                  eq(devices.status, 'active')
                )
              )
          }
        }
      })
    }

    return {
      user,
      apply: options.apply,
      keep: options.keep,
      groups
    }
  } finally {
    await pool.end()
  }
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]
if (isMain) {
  runCli(process.argv.slice(2))
    .then((result) => {
      console.log(JSON.stringify(result, null, 2))
    })
    .catch((error) => {
      console.error(JSON.stringify({ error: error instanceof Error ? error.message : String(error) }, null, 2))
      process.exitCode = 1
    })
}
