// Browser <-> JSON conversion for WebAuthn ceremonies, plus the API-base
// override used to test a separately-hosted build (e.g. a surge.sh
// preview) against a real backend. Everything here is pure/local - no
// endpoint knowledge - so it's shared by the public ceremonies in
// auth-provider.tsx and the authenticated "add a passkey" ceremony in
// api.ts.
//
// The backend (see backend/src/passkey.rs) speaks base64url for every
// binary field - that's what webauthn-rs-core emits/accepts - while
// navigator.credentials.create()/.get() want ArrayBuffers. These helpers
// are the only place that conversion happens.

function base64urlToBuffer(value: string): ArrayBuffer {
  const padded = value + '='.repeat((4 - (value.length % 4)) % 4)
  const base64 = padded.replace(/-/g, '+').replace(/_/g, '/')
  const binary = atob(base64)
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i)
  return bytes.buffer
}

function bufferToBase64url(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer)
  let binary = ''
  for (const byte of bytes) binary += String.fromCharCode(byte)
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

// The backend's CreationChallengeResponse.public_key -> what
// navigator.credentials.create() expects.
export function toCreationOptions(publicKey: any): PublicKeyCredentialCreationOptions {
  return {
    ...publicKey,
    challenge: base64urlToBuffer(publicKey.challenge),
    user: { ...publicKey.user, id: base64urlToBuffer(publicKey.user.id) },
    excludeCredentials: (publicKey.excludeCredentials ?? []).map((c: any) => ({
      ...c,
      id: base64urlToBuffer(c.id),
    })),
  }
}

// The backend's RequestChallengeResponse.public_key -> what
// navigator.credentials.get() expects.
export function toRequestOptions(publicKey: any): PublicKeyCredentialRequestOptions {
  return {
    ...publicKey,
    challenge: base64urlToBuffer(publicKey.challenge),
    allowCredentials: (publicKey.allowCredentials ?? []).map((c: any) => ({
      ...c,
      id: base64urlToBuffer(c.id),
    })),
  }
}

// A browser attestation response (from credentials.create()) -> the JSON
// shape webauthn-rs-core's RegisterPublicKeyCredential parses.
export function attestationToJson(cred: PublicKeyCredential) {
  const response = cred.response as AuthenticatorAttestationResponse
  return {
    id: cred.id,
    rawId: bufferToBase64url(cred.rawId),
    type: cred.type,
    response: {
      attestationObject: bufferToBase64url(response.attestationObject),
      clientDataJSON: bufferToBase64url(response.clientDataJSON),
      transports: response.getTransports?.(),
    },
    extensions: cred.getClientExtensionResults(),
  }
}

// A browser assertion response (from credentials.get()) -> the JSON shape
// webauthn-rs-core's PublicKeyCredential parses.
export function assertionToJson(cred: PublicKeyCredential) {
  const response = cred.response as AuthenticatorAssertionResponse
  return {
    id: cred.id,
    rawId: bufferToBase64url(cred.rawId),
    type: cred.type,
    response: {
      authenticatorData: bufferToBase64url(response.authenticatorData),
      clientDataJSON: bufferToBase64url(response.clientDataJSON),
      signature: bufferToBase64url(response.signature),
      userHandle: response.userHandle ? bufferToBase64url(response.userHandle) : null,
    },
    extensions: cred.getClientExtensionResults(),
  }
}

// `NotAllowedError` covers three distinct browser situations (no
// credential available, user cancelled, ceremony timed out) with no way to
// tell them apart - so this is deliberately vague rather than guessing.
export function describeCeremonyError(e: unknown, label: string): string {
  if (e instanceof DOMException && e.name === 'NotAllowedError') {
    return `${label} was cancelled, timed out, or no passkey was available.`
  }
  return e instanceof Error ? e.message : `${label} failed`
}

const API_BASE_KEY = 'annyeong-api-base'

// Same-origin by default - correct for the normal deployment, where this
// binary serves both the API and the built frontend. VITE_API_BASE is
// this build's own compiled-in override for the opposite case: a build
// deployed to a static host with no backend of its own (e.g.
// hwaiting-demo.surge.sh), baked in at `npm run build` time (see
// hwaiting-demo's install.sh) since there's no server-side process there
// to read an env var from at request time. Below that, `?api=<origin>`
// (persisted to localStorage) remains the manual override for a one-off
// preview pointed at a different backend. This is independent of the
// backend's HWAITING_RP_ID/HWAITING_RP_ORIGINS config, which must match wherever *this page*
// is served from, not wherever the API happens to live - WebAuthn scopes
// the ceremony to the calling page's origin, not the origin its fetch()
// calls go to.
export function apiBase(): string {
  const fromQuery = new URLSearchParams(window.location.search).get('api')
  if (fromQuery) {
    localStorage.setItem(API_BASE_KEY, fromQuery)
  }
  const base = fromQuery || localStorage.getItem(API_BASE_KEY) || import.meta.env.VITE_API_BASE || window.location.origin
  return base.replace(/\/$/, '')
}
