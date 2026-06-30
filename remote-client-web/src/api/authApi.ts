import type { AuthUser } from '../auth/authStore.js'
import type { HttpClient } from './httpClient.js'

export type LoginResponse = {
  access_token: string
  refresh_token: string
  expires_at: string
  user: AuthUser
}

export type RefreshResponse = {
  access_token: string
  refresh_token: string
  expires_at: string
  user: AuthUser
}

export function createAuthApi(http: HttpClient) {
  return {
    login(email: string, password: string) {
      return http.post<LoginResponse>('/api/v1/auth/login', { email, password })
    },
    refresh(refreshToken: string) {
      return http.post<RefreshResponse>('/api/v1/auth/refresh', { refresh_token: refreshToken })
    },
    logout(refreshToken: string) {
      return http.post<null>('/api/v1/auth/logout', { refresh_token: refreshToken })
    },
    me() {
      return http.get<AuthUser>('/api/v1/auth/me')
    }
  }
}
