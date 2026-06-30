#!/usr/bin/env node

export function buildSmokeConfig(env = process.env) {
  return {
    remoteServerUrl: env.REMOTE_SERVER_URL ?? 'http://127.0.0.1:27880',
    remoteClientWebUrl: env.REMOTE_CLIENT_WEB_URL ?? 'http://127.0.0.1:27883',
    corsOrigin: env.REMOTE_CLIENT_CORS_ORIGIN ?? 'http://localhost:27883',
    userEmail: env.SMOKE_USER_EMAIL,
    userPassword: env.SMOKE_USER_PASSWORD,
    adminEmail: env.SMOKE_ADMIN_EMAIL,
    adminPassword: env.SMOKE_ADMIN_PASSWORD,
    expectUserAdminForbidden: env.SMOKE_EXPECT_USER_ADMIN_FORBIDDEN === 'true'
  }
}

function joinUrl(baseUrl, path) {
  return `${baseUrl.replace(/\/$/, '')}/${path.replace(/^\//, '')}`
}

async function readJson(response, label) {
  const text = await response.text()
  try {
    return JSON.parse(text)
  } catch {
    throw new Error(`${label} returned invalid JSON`)
  }
}

export async function expectEnvelopeOk(response, label) {
  if (!response.ok) throw new Error(`${label} http failed: ${response.status}`)
  const payload = await readJson(response, label)
  if (payload.code !== 0) {
    throw new Error(`${label} failed: ${payload.code} ${payload.message}`)
  }
  return payload.data
}

async function postJson(fetchImpl, url, body, headers = {}) {
  return fetchImpl(url, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      ...headers
    },
    body: JSON.stringify(body)
  })
}

async function checkHealth(fetchImpl, config) {
  await expectEnvelopeOk(await fetchImpl(joinUrl(config.remoteServerUrl, '/api/v1/health')), 'health')
  return 'remote-server health ok'
}

async function checkCors(fetchImpl, config) {
  const response = await fetchImpl(joinUrl(config.remoteServerUrl, '/api/v1/auth/login'), {
    method: 'OPTIONS',
    headers: {
      Origin: config.corsOrigin,
      'Access-Control-Request-Method': 'POST'
    }
  })
  const allowedOrigin = response.headers.get('access-control-allow-origin')
  if (response.status !== 204 || allowedOrigin !== config.corsOrigin) {
    throw new Error(`cors preflight failed: status=${response.status} allow-origin=${allowedOrigin}`)
  }
  return 'cors preflight ok'
}

async function checkRemoteClientWeb(fetchImpl, config) {
  const response = await fetchImpl(config.remoteClientWebUrl)
  if (!response.ok) throw new Error(`remote-client-web http failed: ${response.status}`)
  const html = await response.text()
  if (!html.includes('<html')) throw new Error('remote-client-web returned non-html response')
  return 'remote-client-web reachable'
}

async function loginUser(fetchImpl, config) {
  if (!config.userEmail || !config.userPassword) return null
  const data = await expectEnvelopeOk(
    await postJson(fetchImpl, joinUrl(config.remoteServerUrl, '/api/v1/auth/login'), {
      email: config.userEmail,
      password: config.userPassword
    }),
    'user login'
  )
  if (!data?.access_token) throw new Error('user login response missing access_token')
  return data.access_token
}

async function checkDeviceList(fetchImpl, config, accessToken) {
  await expectEnvelopeOk(
    await fetchImpl(joinUrl(config.remoteServerUrl, '/api/v1/devices/list'), {
      headers: {
        Authorization: `Bearer ${accessToken}`
      }
    }),
    'device list'
  )
  return 'device list ok'
}

async function checkUserAdminBoundary(fetchImpl, config) {
  if (!config.expectUserAdminForbidden || !config.userEmail || !config.userPassword) return null
  const response = await postJson(fetchImpl, joinUrl(config.remoteServerUrl, '/api/v1/admin/auth/login'), {
    email: config.userEmail,
    password: config.userPassword
  })
  const payload = await readJson(response, 'normal user admin boundary')
  if (payload.code !== 230401) {
    throw new Error(`normal user admin boundary failed: ${payload.code} ${payload.message}`)
  }
  return 'normal user admin boundary ok'
}

async function checkAdminLogin(fetchImpl, config) {
  if (!config.adminEmail || !config.adminPassword) return null
  const data = await expectEnvelopeOk(
    await postJson(fetchImpl, joinUrl(config.remoteServerUrl, '/api/v1/admin/auth/login'), {
      email: config.adminEmail,
      password: config.adminPassword
    }),
    'admin login'
  )
  if (data?.user?.role !== 'admin') throw new Error('admin login response missing admin user')
  return 'admin login ok'
}

export async function runRemoteSmokeChecks(options = {}) {
  const config = buildSmokeConfig(options.env ?? process.env)
  const fetchImpl = options.fetchImpl ?? globalThis.fetch
  if (typeof fetchImpl !== 'function') throw new Error('fetch is not available')

  const results = [
    await checkHealth(fetchImpl, config),
    await checkCors(fetchImpl, config),
    await checkRemoteClientWeb(fetchImpl, config)
  ]

  const userAccessToken = await loginUser(fetchImpl, config)
  if (userAccessToken) {
    results.push('user login ok')
    results.push(await checkDeviceList(fetchImpl, config, userAccessToken))
  }

  const userBoundary = await checkUserAdminBoundary(fetchImpl, config)
  if (userBoundary) results.push(userBoundary)

  const adminLogin = await checkAdminLogin(fetchImpl, config)
  if (adminLogin) results.push(adminLogin)

  return results
}

async function main() {
  try {
    const results = await runRemoteSmokeChecks()
    for (const result of results) {
      console.log(`ok - ${result}`)
    }
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error))
    process.exitCode = 1
  }
}

if (import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  await main()
}
