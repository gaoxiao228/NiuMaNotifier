import type { HttpClient } from './httpClient.js'

export type AuthUser = {
  id: string
  email: string
  role: 'admin' | 'user'
  status: 'active' | 'disabled'
}

export type LoginResponse = {
  access_token: string
  refresh_token: string
  expires_at: string
  user: AuthUser
}

export function createAuthApi(http: HttpClient) {
  return {
    login(email: string, password: string) {
      return http.post<LoginResponse>('/api/v1/auth/login', { email, password })
    }
  }
}
