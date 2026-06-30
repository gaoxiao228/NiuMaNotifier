export const supportedLanguages = ['zh-CN', 'zh-TW', 'en', 'ja', 'ko', 'de'] as const
export type SupportedLanguage = (typeof supportedLanguages)[number]

export const messages = {
  'zh-CN': {
    app_title: 'NiuMa Remote Client',
    login_title: '登录远程客户端',
    login_description: '使用远程访问凭证进入你的设备会话。',
    login_button: '继续',
    endpoint_label: '服务地址',
    endpoint_placeholder: 'https://remote.example.com',
    token_label: '访问令牌',
    token_placeholder: '请输入访问令牌'
  },
  'zh-TW': {
    app_title: 'NiuMa Remote Client',
    login_title: '登入遠端用戶端',
    login_description: '使用遠端存取憑證進入你的裝置會話。',
    login_button: '繼續',
    endpoint_label: '服務位址',
    endpoint_placeholder: 'https://remote.example.com',
    token_label: '存取權杖',
    token_placeholder: '請輸入存取權杖'
  },
  en: {
    app_title: 'NiuMa Remote Client',
    login_title: 'Sign in to remote client',
    login_description: 'Use remote access credentials to enter your device session.',
    login_button: 'Continue',
    endpoint_label: 'Service URL',
    endpoint_placeholder: 'https://remote.example.com',
    token_label: 'Access token',
    token_placeholder: 'Enter access token'
  },
  ja: {
    app_title: 'NiuMa Remote Client',
    login_title: 'リモートクライアントにサインイン',
    login_description: 'リモートアクセス資格情報でデバイスセッションに入ります。',
    login_button: '続行',
    endpoint_label: 'サービス URL',
    endpoint_placeholder: 'https://remote.example.com',
    token_label: 'アクセストークン',
    token_placeholder: 'アクセストークンを入力'
  },
  ko: {
    app_title: 'NiuMa Remote Client',
    login_title: '원격 클라이언트 로그인',
    login_description: '원격 접근 자격 증명으로 기기 세션에 들어갑니다.',
    login_button: '계속',
    endpoint_label: '서비스 URL',
    endpoint_placeholder: 'https://remote.example.com',
    token_label: '접근 토큰',
    token_placeholder: '접근 토큰 입력'
  },
  de: {
    app_title: 'NiuMa Remote Client',
    login_title: 'Beim Remote-Client anmelden',
    login_description: 'Mit Remote-Zugriffsdaten die Gerätesitzung öffnen.',
    login_button: 'Weiter',
    endpoint_label: 'Service-URL',
    endpoint_placeholder: 'https://remote.example.com',
    token_label: 'Zugriffstoken',
    token_placeholder: 'Zugriffstoken eingeben'
  }
} satisfies Record<SupportedLanguage, Record<string, string>>

export type MessageKey = keyof (typeof messages)['en']
