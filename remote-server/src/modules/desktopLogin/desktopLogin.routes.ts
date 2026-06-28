import type { FastifyInstance } from 'fastify'
import { loadConfigFromEnv } from '../../config.js'
import { createDb } from '../../db/client.js'
import { ErrorCode } from '../../shared/errors.js'
import { apiFailure, apiSuccess } from '../../shared/response.js'
import { parseBody } from '../../shared/validation.js'
import { requireAuth } from '../auth/auth.middleware.js'
import { createAuthRepository } from '../auth/auth.repository.js'
import {
  desktopLoginCompleteSchema,
  desktopLoginPollSchema,
  desktopLoginStartSchema
} from './desktopLogin.schemas.js'
import { createDesktopLoginRepository } from './desktopLogin.repository.js'
import { createDesktopLoginService } from './desktopLogin.service.js'

function escapeHtml(value: string) {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;')
}

export function renderDesktopLoginPage(requestId: string) {
  const safeRequestId = escapeHtml(requestId)
  return `<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>NiuMaNotifier</title>
  <style>
    :root { color-scheme: light; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }
    body { margin: 0; min-height: 100vh; display: grid; place-items: center; background: #f5f7fb; color: #172033; }
    main { width: min(420px, calc(100vw - 32px)); background: #fff; border: 1px solid #d7dfef; border-radius: 8px; padding: 24px; box-sizing: border-box; }
    h1 { margin: 0 0 8px; font-size: 22px; line-height: 1.25; }
    p { margin: 0 0 18px; color: #516079; line-height: 1.5; }
    label { display: grid; gap: 6px; margin: 14px 0; font-size: 14px; font-weight: 600; }
    input { height: 40px; border: 1px solid #cbd6ea; border-radius: 6px; padding: 0 10px; font: inherit; }
    .actions { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; margin-top: 18px; }
    button { height: 40px; border: 1px solid #2f6fed; border-radius: 6px; background: #2f6fed; color: white; font: inherit; font-weight: 700; cursor: pointer; }
    button.secondary { background: white; color: #2f6fed; }
    button:disabled { opacity: .65; cursor: default; }
    output { display: block; min-height: 22px; margin-top: 16px; color: #40506c; font-size: 14px; line-height: 1.5; }
  </style>
</head>
<body>
  <main>
    <h1 data-i18n="title">NiuMaNotifier 登录绑定</h1>
    <p data-i18n="summary">登录账号后，将这台设备绑定到远程服务端。</p>
    <form id="desktop-login-form">
      <input type="hidden" id="request-id" value="${safeRequestId}">
      <label>
        <span data-i18n="email">邮箱</span>
        <input id="email" name="email" type="email" autocomplete="username" required>
      </label>
      <label>
        <span data-i18n="password">密码</span>
        <input id="password" name="password" type="password" autocomplete="current-password" minlength="8" required>
      </label>
      <div class="actions">
        <button id="login" type="submit" data-mode="login" data-i18n="login">登录并绑定</button>
        <button class="secondary" id="register" type="button" data-mode="register" data-i18n="register">注册并绑定</button>
      </div>
      <output id="result" role="status"></output>
    </form>
  </main>
  <script>
    const messages = {
      'zh-CN': { title: 'NiuMaNotifier 登录绑定', summary: '登录账号后，将这台设备绑定到远程服务端。', email: '邮箱', password: '密码', login: '登录并绑定', register: '注册并绑定', working: '处理中...', done: '绑定完成，可以回到 NiuMaNotifier。' },
      'zh-TW': { title: 'NiuMaNotifier 登入綁定', summary: '登入帳號後，將這台裝置綁定到遠端服務端。', email: '電子郵件', password: '密碼', login: '登入並綁定', register: '註冊並綁定', working: '處理中...', done: '綁定完成，可以回到 NiuMaNotifier。' },
      en: { title: 'NiuMaNotifier Sign In', summary: 'Sign in to bind this device to the remote server.', email: 'Email', password: 'Password', login: 'Sign in and bind', register: 'Register and bind', working: 'Working...', done: 'Binding complete. Return to NiuMaNotifier.' },
      ja: { title: 'NiuMaNotifier ログイン連携', summary: 'アカウントにログインして、このデバイスをリモートサーバーに連携します。', email: 'メール', password: 'パスワード', login: 'ログインして連携', register: '登録して連携', working: '処理中...', done: '連携が完了しました。NiuMaNotifier に戻ってください。' },
      ko: { title: 'NiuMaNotifier 로그인 바인딩', summary: '계정에 로그인해 이 기기를 원격 서버에 바인딩합니다.', email: '이메일', password: '비밀번호', login: '로그인 및 바인딩', register: '가입 및 바인딩', working: '처리 중...', done: '바인딩이 완료되었습니다. NiuMaNotifier로 돌아가세요.' },
      de: { title: 'NiuMaNotifier Anmeldung', summary: 'Melden Sie sich an, um dieses Gerät mit dem Remote-Server zu verbinden.', email: 'E-Mail', password: 'Passwort', login: 'Anmelden und verbinden', register: 'Registrieren und verbinden', working: 'Wird verarbeitet...', done: 'Verbindung abgeschlossen. Zurück zu NiuMaNotifier.' }
    };
    const language = navigator.language in messages ? navigator.language : (navigator.language.startsWith('zh-TW') ? 'zh-TW' : navigator.language.split('-')[0]);
    const t = messages[language] || messages.en;
    document.querySelectorAll('[data-i18n]').forEach((node) => { node.textContent = t[node.dataset.i18n] || node.textContent; });
    const form = document.getElementById('desktop-login-form');
    const result = document.getElementById('result');
    const requestId = document.getElementById('request-id').value;
    async function postJson(url, body, token) {
      const response = await fetch(url, {
        method: 'POST',
        headers: { 'content-type': 'application/json', ...(token ? { authorization: 'Bearer ' + token } : {}) },
        body: JSON.stringify(body)
      });
      const payload = await response.json();
      if (payload.code !== 0) throw new Error(payload.message);
      return payload.data;
    }
    async function bind(mode) {
      const email = document.getElementById('email').value;
      const password = document.getElementById('password').value;
      result.textContent = t.working;
      if (mode === 'register') await postJson('/api/v1/auth/register', { email, password });
      const session = await postJson('/api/v1/auth/login', { email, password });
      await postJson('/api/v1/desktop-login/complete', { request_id: requestId }, session.access_token);
      result.textContent = t.done;
    }
    form.addEventListener('submit', async (event) => {
      event.preventDefault();
      try { await bind('login'); } catch (error) { result.textContent = error.message; }
    });
    document.getElementById('register').addEventListener('click', async () => {
      try { await bind('register'); } catch (error) { result.textContent = error.message; }
    });
  </script>
</body>
</html>`
}

export async function registerDesktopLoginRoutes(app: FastifyInstance) {
  const config = loadConfigFromEnv()
  const { db } = createDb(config.databaseUrl)
  const authRepo = createAuthRepository(db)
  const service = createDesktopLoginService({
    repo: createDesktopLoginRepository(db),
    config: {
      publicUrl: config.publicUrl,
      tokenPepper: config.tokenPepper,
      desktopLoginTtlSeconds: 600
    }
  })

  app.post('/api/v1/desktop-login/start', async (request) => {
    const parsed = parseBody(desktopLoginStartSchema, request.body)
    if (!parsed.ok) return parsed.response

    const result = await service.start(parsed.data)
    return apiSuccess(result.data)
  })

  app.get('/desktop-login', async (request, reply) => {
    const query = request.query as { request_id?: string }
    const requestId = typeof query.request_id === 'string' ? query.request_id : ''
    if (!requestId) return reply.status(400).send(apiFailure(ErrorCode.PROTOCOL_MISSING_REQUIRED, 'request_id不能为空'))
    return reply.type('text/html; charset=utf-8').send(renderDesktopLoginPage(requestId))
  })

  app.post('/api/v1/desktop-login/complete', async (request) => {
    const auth = await requireAuth(request, config.jwtPublicKey)
    if (!auth.ok) return auth.response

    const parsed = parseBody(desktopLoginCompleteSchema, request.body)
    if (!parsed.ok) return parsed.response

    const currentUser = await authRepo.findUserById(auth.auth.userId)
    if (!currentUser || currentUser.status !== 'active') return apiFailure(ErrorCode.UNAUTHORIZED, '未登录')

    const result = await service.complete({
      requestId: parsed.data.request_id,
      user: {
        id: currentUser.id,
        email: currentUser.email,
        role: currentUser.role
      }
    })
    return result.ok ? apiSuccess(result.data) : apiFailure(result.code, result.message)
  })

  app.post('/api/v1/desktop-login/poll', async (request) => {
    const parsed = parseBody(desktopLoginPollSchema, request.body)
    if (!parsed.ok) return parsed.response

    const result = await service.poll(parsed.data)
    return result.ok ? apiSuccess(result.data) : apiFailure(result.code, result.message, result.data ?? null)
  })
}
