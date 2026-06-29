import { apiFetch } from './http'

export interface AuthCheckResponse { ok: boolean }
export interface LoginInput { accessKey: string; secretKey: string }

export interface KeycloakConfigResponse {
  enabled: boolean
  realm?: string
  clientId?: string
}

export interface KeycloakTokenResponse {
  access_token: string
  expires_in: number
  refresh_expires_in: number
  refresh_token: string
  token_type: string
  scope: string
}

export interface KeycloakLoginInput {
  username: string
  password: string
}

export async function checkAuth(): Promise<AuthCheckResponse> {
  return apiFetch<AuthCheckResponse>('/api/auth/check')
}

export async function login(input: LoginInput): Promise<AuthCheckResponse> {
  return apiFetch<AuthCheckResponse>('/api/auth/login', {
    method: 'POST',
    body: JSON.stringify(input),
  })
}

export async function logout(): Promise<AuthCheckResponse> {
  return apiFetch<AuthCheckResponse>('/api/auth/logout', { method: 'POST' })
}

export async function getKeycloakConfig(): Promise<KeycloakConfigResponse> {
  return apiFetch<KeycloakConfigResponse>('/api/auth/keycloak-config')
}

export async function keycloakLogin(input: KeycloakLoginInput): Promise<KeycloakTokenResponse> {
  return apiFetch<KeycloakTokenResponse>('/api/auth/keycloak-login', {
    method: 'POST',
    body: JSON.stringify(input),
  })
}

export async function keycloakRefresh(): Promise<KeycloakTokenResponse> {
  return apiFetch<KeycloakTokenResponse>('/api/auth/keycloak-refresh', {
    method: 'POST',
    body: JSON.stringify({}),
  })
}