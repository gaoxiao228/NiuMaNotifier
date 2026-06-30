import { LoginOutlined } from '@ant-design/icons'
import { Alert, Button, Form, Input } from 'antd'
import { useI18n } from '../i18n/index.js'

export type LoginPageProps = {
  loading?: boolean
  error?: string | null
  onLogin: (email: string, password: string) => Promise<void> | void
}

type LoginFormValues = {
  email: string
  password: string
}

export function LoginPage({ loading = false, error, onLogin }: LoginPageProps) {
  const { t } = useI18n()

  function handleFinish(values: LoginFormValues) {
    return onLogin(values.email, values.password)
  }

  return (
    <section className="login-page" aria-labelledby="remote-client-title">
      <div className="login-copy">
        <h1 id="remote-client-title">{t('app_title')}</h1>
        <p>{t('login_description')}</p>
      </div>

      <Form<LoginFormValues> className="login-form" layout="vertical" autoComplete="off" onFinish={handleFinish}>
        <h2>{t('login_title')}</h2>
        {error ? <Alert className="form-alert" type="error" message={error} showIcon /> : null}
        <Form.Item
          label={t('email_label')}
          name="email"
          rules={[{ required: true, message: t('email_required') }]}
        >
          <Input type="email" placeholder={t('email_placeholder')} autoComplete="email" />
        </Form.Item>
        <Form.Item
          label={t('password_label')}
          name="password"
          rules={[{ required: true, message: t('password_required') }]}
        >
          <Input.Password placeholder={t('password_placeholder')} autoComplete="current-password" />
        </Form.Item>
        <Button type="primary" htmlType="submit" icon={<LoginOutlined />} loading={loading}>
          {t('login_button')}
        </Button>
      </Form>
    </section>
  )
}
