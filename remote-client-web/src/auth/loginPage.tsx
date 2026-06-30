import { LoginOutlined } from '@ant-design/icons'
import { Bubble } from '@ant-design/x'
import { Button, Form, Input } from 'antd'
import { useI18n } from '../i18n/index.js'

export function LoginPage() {
  const { t } = useI18n()

  return (
    <section className="login-page" aria-labelledby="remote-client-title">
      <div className="login-copy">
        <h1 id="remote-client-title">{t('app_title')}</h1>
        <Bubble content={t('login_description')} placement="start" />
      </div>

      <Form className="login-form" layout="vertical" autoComplete="off">
        <h2>{t('login_title')}</h2>
        <Form.Item label={t('endpoint_label')} name="endpoint">
          <Input placeholder={t('endpoint_placeholder')} />
        </Form.Item>
        <Form.Item label={t('token_label')} name="token">
          <Input.Password placeholder={t('token_placeholder')} />
        </Form.Item>
        <Button type="primary" htmlType="submit" icon={<LoginOutlined />}>
          {t('login_button')}
        </Button>
      </Form>
    </section>
  )
}
