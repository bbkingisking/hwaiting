package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"time"
)

// apiError wraps a non-2xx response. The server always answers errors with
// {"error": "..."} (see backend/src/error.rs), so we surface that message
// rather than a raw status code.
type apiError struct {
	status int
	body   []byte
}

func (e *apiError) Error() string {
	var parsed struct {
		Error string `json:"error"`
	}
	if json.Unmarshal(e.body, &parsed) == nil && parsed.Error != "" {
		return fmt.Sprintf("%s (HTTP %d)", parsed.Error, e.status)
	}
	return fmt.Sprintf("HTTP %d: %s", e.status, string(e.body))
}

var httpClient = &http.Client{Timeout: 30 * time.Second}

// request performs an API call. When token is non-empty it's sent as a
// bearer token. Returns the raw response body on 2xx, or *apiError on
// anything else - callers never need to inspect status codes themselves.
func request(method, path, token string, body []byte) ([]byte, error) {
	req, err := http.NewRequest(method, baseURL()+path, bytes.NewReader(body))
	if err != nil {
		return nil, err
	}
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	if token != "" {
		req.Header.Set("Authorization", "Bearer "+token)
	}

	resp, err := httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("could not reach %s: %w", baseURL(), err)
	}
	defer resp.Body.Close()

	respBody, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, err
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return nil, &apiError{status: resp.StatusCode, body: respBody}
	}
	return respBody, nil
}

// login exchanges username/password for a JWT and saves it to disk for
// every later command to read. Only the fields needed to confirm success
// and persist the token are decoded - everything else about the response
// shape is the server's business, not this client's.
func login(username, password string) ([]byte, error) {
	reqBody, err := json.Marshal(map[string]string{
		"username": username,
		"password": password,
	})
	if err != nil {
		return nil, err
	}

	respBody, err := request(http.MethodPost, "/api/auth/login", "", reqBody)
	if err != nil {
		return nil, err
	}

	var parsed struct {
		Token    string `json:"token"`
		Username string `json:"username"`
		IsAdmin  bool   `json:"is_admin"`
	}
	if err := json.Unmarshal(respBody, &parsed); err != nil {
		return nil, fmt.Errorf("malformed login response: %w", err)
	}
	if err := saveToken(parsed.Token); err != nil {
		return nil, fmt.Errorf("logged in but could not save token: %w", err)
	}

	// Deliberately doesn't echo the token back - it's already on disk and
	// has no further use in the transcript.
	return json.Marshal(map[string]any{
		"status":   "ok",
		"username": parsed.Username,
		"is_admin": parsed.IsAdmin,
	})
}

// review fetches the next due card. The response is passed through
// verbatim - see the "raw JSON" note in cmd_review's doc comment.
func review() ([]byte, error) {
	token, err := loadToken()
	if err != nil {
		return nil, fmt.Errorf("not logged in (run `login` first): %w", err)
	}
	return request(http.MethodGet, "/api/cards/next", token, nil)
}

// answer submits a guess for the given card and returns the graded result
// (CheckResponse: correct + the CardReveal fields), passed through verbatim.
func answer(cardID, guess string) ([]byte, error) {
	token, err := loadToken()
	if err != nil {
		return nil, fmt.Errorf("not logged in (run `login` first): %w", err)
	}
	reqBody, err := json.Marshal(map[string]string{"answer": guess})
	if err != nil {
		return nil, err
	}
	return request(http.MethodPost, "/api/cards/"+cardID+"/check", token, reqBody)
}

// comment records a content-review note against a card server-side.
func comment(cardID, body string) ([]byte, error) {
	token, err := loadToken()
	if err != nil {
		return nil, fmt.Errorf("not logged in (run `login` first): %w", err)
	}
	reqBody, err := json.Marshal(map[string]string{"body": body})
	if err != nil {
		return nil, err
	}
	return request(http.MethodPost, "/api/cards/"+cardID+"/comment", token, reqBody)
}

// lookups fetches the current pos/origin_type/grade/speech_level/tense/
// grammar_pattern tables live, so the caller never has to keep a hardcoded
// copy in sync by hand.
func lookups() ([]byte, error) {
	token, err := loadToken()
	if err != nil {
		return nil, fmt.Errorf("not logged in (run `login` first): %w", err)
	}
	return request(http.MethodGet, "/api/cards/enum-lookups", token, nil)
}
