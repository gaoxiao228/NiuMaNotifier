import pg from 'pg'
import { fileURLToPath } from 'node:url'

export function planDeviceDedupe(rows) {
  const rowsByName = new Map()

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

function compareRowsForKeep(left, right) {
  const timeDiff = Date.parse(right.created_at) - Date.parse(left.created_at)
  if (timeDiff !== 0) return timeDiff
  return left.id.localeCompare(right.id)
}

function parseArgs(argv) {
  let userEmail
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

export async function runDeviceDedupe(argv, env = process.env) {
  const options = parseArgs(argv)
  const databaseUrl = env.DATABASE_URL
  if (!databaseUrl) {
    throw new Error('必须配置 DATABASE_URL')
  }

  const pool = new pg.Pool({ connectionString: databaseUrl })
  try {
    const user = (
      await pool.query('select id, email from users where email = $1 limit 1', [
        options.userEmail
      ])
    ).rows[0]
    if (!user) {
      throw new Error(`未找到用户：${options.userEmail}`)
    }

    const activeDevices = (
      await pool.query(
        'select id, name, created_at from devices where user_id = $1 and status = $2',
        [user.id, 'active']
      )
    ).rows

    const groups = planDeviceDedupe(
      activeDevices.map((device) => ({
        id: device.id,
        name: device.name,
        created_at:
          device.created_at instanceof Date
            ? device.created_at.toISOString()
            : String(device.created_at)
      }))
    )

    if (options.apply) {
      const client = await pool.connect()
      try {
        await client.query('begin')
        const ids = groups.flatMap((group) => group.revoke.map((device) => device.id))
        if (ids.length > 0) {
          // 只标记当前用户仍处于 active 的重复设备，不做物理删除。
          await client.query(
            'update devices set status = $1, revoked_at = now(), updated_at = now() where user_id = $2 and status = $3 and id = any($4::uuid[])',
            ['revoked', user.id, 'active', ids]
          )
        }
        await client.query('commit')
      } catch (error) {
        await client.query('rollback')
        throw error
      } finally {
        client.release()
      }
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
  runDeviceDedupe(process.argv.slice(2))
    .then((result) => {
      console.log(JSON.stringify(result, null, 2))
    })
    .catch((error) => {
      console.error(JSON.stringify({ error: error instanceof Error ? error.message : String(error) }))
      process.exitCode = 1
    })
}
