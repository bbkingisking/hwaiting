#!/usr/bin/env bash
#
# create_user_set_limit.sh
#
# 1. Logs in as admin -> gets admin JWT
# 2. Generates a single invite code (admin-only endpoint)
# 3. Signs up a new user with a random username/password -> gets the new user's JWT
# 4. PATCHes the new user's settings to set daily_new_card_limit=2000
# 5. Prints the generated username/password to stdout
#
# NOTE: this API has no numeric "user ID" anywhere in its schema.
# UserProfile only exposes `username`, and AuthResponse only exposes
# token/username/is_admin. The new user's *username* + *token* are the
# only handles you get -- there's no separate ID to fetch. Settings are
# always scoped to "whoever's bearer token this is", so we have to use
# the new user's own token (not the admin's) to touch their settings.
#
# Usage:
#   ./create_user_set_limit.sh <admin_username> <admin_password>
#
# Example:
#   ./create_user_set_limit.sh admin s3cr3t

set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "Usage: $0 <admin_username> <admin_password>" >&2
  exit 1
fi

BASE_URL="http://localhost:15000"
ADMIN_USER="$1"
ADMIN_PASS="$2"
NEW_DAILY_LIMIT=2000

rand_str() {
  # alnum only, no ambiguous chars needed since it's machine-generated
  #
  # NOTE: bound the /dev/urandom read with an upstream head first, so tr
  # hits a natural EOF. Without it (`tr ... | head -c N`), the downstream
  # head closes the pipe as soon as it has N bytes, tr gets SIGPIPE (exit
  # 141), and pipefail+set -e kill the whole script silently before it
  # logs anything.
  head -c 4096 /dev/urandom | tr -dc 'A-Za-z0-9' | head -c "$1"
}

NEW_USER="user_$(rand_str 8)"
NEW_PASS="$(rand_str 20)"

need() { command -v "$1" >/dev/null 2>&1 || { echo "Missing dependency: $1" >&2; exit 1; }; }
need curl
need jq

# --- helper: do a request, dump body+status, bail on HTTP error -------------
req() {
  local method="$1" url="$2" data="${3:-}" token="${4:-}"
  local -a curl_args=(-sS -w '\n%{http_code}' -X "$method" "$url" -H 'Content-Type: application/json')
  [[ -n "$token" ]] && curl_args+=(-H "Authorization: Bearer $token")
  [[ -n "$data" ]] && curl_args+=(-d "$data")

  local raw status body
  raw="$(curl "${curl_args[@]}")"
  status="${raw##*$'\n'}"
  body="${raw%$'\n'*}"

  if [[ "$status" -lt 200 || "$status" -ge 300 ]]; then
    echo "Request failed: $method $url (HTTP $status)" >&2
    echo "$body" >&2
    exit 1
  fi
  echo "$body"
}

echo "== 1. Logging in as admin ($ADMIN_USER) ==" >&2
admin_login_payload=$(jq -n --arg u "$ADMIN_USER" --arg p "$ADMIN_PASS" '{username:$u,password:$p}')
admin_auth=$(req POST "$BASE_URL/api/auth/login" "$admin_login_payload")

admin_token=$(jq -r '.token' <<<"$admin_auth")
admin_is_admin=$(jq -r '.is_admin' <<<"$admin_auth")

if [[ "$admin_is_admin" != "true" ]]; then
  echo "Error: '$ADMIN_USER' logged in fine but is_admin=false; admin endpoints will 403." >&2
  exit 1
fi
echo "  -> got admin token" >&2

echo "== 2. Generating one invite code ==" >&2
invite_payload='{"count":1}'
invites_resp=$(req POST "$BASE_URL/api/admin/invites" "$invite_payload" "$admin_token")
invite_code=$(jq -r '.codes[0].code' <<<"$invites_resp")

if [[ -z "$invite_code" || "$invite_code" == "null" ]]; then
  echo "Error: no invite code came back. Raw response:" >&2
  echo "$invites_resp" >&2
  exit 1
fi
echo "  -> invite code: $invite_code" >&2

echo "== 3. Signing up new user ($NEW_USER) ==" >&2
signup_payload=$(jq -n --arg u "$NEW_USER" --arg p "$NEW_PASS" --arg c "$invite_code" \
  '{username:$u,password:$p,invite_code:$c}')
new_auth=$(req POST "$BASE_URL/api/auth/signup" "$signup_payload")

new_token=$(jq -r '.token' <<<"$new_auth")
new_username=$(jq -r '.username' <<<"$new_auth")
echo "  -> new user created: $new_username" >&2
echo "  -> (this API exposes no numeric user ID -- username + token are the only identifiers)" >&2

echo "== 4. Setting daily_new_card_limit=$NEW_DAILY_LIMIT for $new_username ==" >&2
settings_payload=$(jq -n --argjson lim "$NEW_DAILY_LIMIT" '{daily_new_card_limit:$lim}')
settings_resp=$(req PATCH "$BASE_URL/api/user/settings" "$settings_payload" "$new_token")

echo "== Done ==" >&2
echo "$settings_resp" | jq . >&2

# Credentials for the newly created user go to stdout (only this).
echo "username: $new_username"
echo "password: $NEW_PASS"
