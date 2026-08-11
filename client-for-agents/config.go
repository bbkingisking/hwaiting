package main

import (
	"os"
	"path/filepath"
)

// defaultBaseURL is used when HWAITING_API_URL isn't set. Matches the port
// prod listens on; override for local dev or a different deployment.
const defaultBaseURL = "http://localhost:15000"

func baseURL() string {
	if v := os.Getenv("HWAITING_API_URL"); v != "" {
		return v
	}
	return defaultBaseURL
}

// stateDir holds the JWT saved by `login` and read by every other command.
// Not the same env vars a browser session would use - this is a standing
// dummy account whose token just needs to survive between separate process
// invocations, since each `hwaiting-agent` call starts cold.
func stateDir() (string, error) {
	base := os.Getenv("XDG_CONFIG_HOME")
	if base == "" {
		home, err := os.UserHomeDir()
		if err != nil {
			return "", err
		}
		base = filepath.Join(home, ".config")
	}
	dir := filepath.Join(base, "hwaiting-agent")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return "", err
	}
	return dir, nil
}

func tokenPath() (string, error) {
	dir, err := stateDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(dir, "token"), nil
}

func saveToken(token string) error {
	path, err := tokenPath()
	if err != nil {
		return err
	}
	return os.WriteFile(path, []byte(token), 0o600)
}

func loadToken() (string, error) {
	path, err := tokenPath()
	if err != nil {
		return "", err
	}
	b, err := os.ReadFile(path)
	if err != nil {
		return "", err
	}
	return string(b), nil
}

// dataDir holds the comment log. Kept separate from stateDir/XDG_CONFIG_HOME
// on principle (config vs. data), though both currently resolve under $HOME
// when no XDG vars are set.
func dataDir() (string, error) {
	base := os.Getenv("XDG_DATA_HOME")
	if base == "" {
		home, err := os.UserHomeDir()
		if err != nil {
			return "", err
		}
		base = filepath.Join(home, ".local", "share")
	}
	dir := filepath.Join(base, "hwaiting-agent")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return "", err
	}
	return dir, nil
}

func commentLogPath() (string, error) {
	dir, err := dataDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(dir, "comments.jsonl"), nil
}
