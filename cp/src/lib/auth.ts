const KEY = 'apex_cp_token'

export function getToken(): string | null {
  return sessionStorage.getItem(KEY) ?? localStorage.getItem(KEY)
}

export function setToken(token: string, persist: boolean) {
  if (persist) {
    localStorage.setItem(KEY, token)
    sessionStorage.removeItem(KEY)
  } else {
    sessionStorage.setItem(KEY, token)
    localStorage.removeItem(KEY)
  }
}

export function clearToken() {
  sessionStorage.removeItem(KEY)
  localStorage.removeItem(KEY)
}

export function authHeaders(): Record<string, string> {
  const t = getToken()
  return t ? { Authorization: `Bearer ${t}` } : {}
}
