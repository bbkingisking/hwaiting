// hwaiting-agent is a thin CLI over the hwaiting review API, meant to be
// driven by an agent (see SKILL.md) rather than a human. Every command
// prints one JSON value to stdout on success and exits 0; on failure it
// prints a plain-text message to stderr and exits non-zero. Nothing here
// interprets card content - that's the skill's job, not this binary's.
package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
)

func main() {
	if len(os.Args) < 2 {
		usage()
		os.Exit(2)
	}

	var (
		out []byte
		err error
	)

	switch os.Args[1] {
	case "login":
		out, err = cmdLogin(os.Args[2:])
	case "review":
		out, err = cmdReview(os.Args[2:])
	case "answer":
		out, err = cmdAnswer(os.Args[2:])
	case "comment":
		out, err = cmdComment(os.Args[2:])
	case "lookups":
		out, err = cmdLookups(os.Args[2:])
	case "-h", "--help", "help":
		usage()
		return
	default:
		fmt.Fprintf(os.Stderr, "unknown command %q\n", os.Args[1])
		usage()
		os.Exit(2)
	}

	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	printJSON(out)
}

func usage() {
	fmt.Fprint(os.Stderr, `hwaiting-agent - CLI review client for hwaiting

Usage:
  hwaiting-agent login
  hwaiting-agent review
  hwaiting-agent answer <card_id> <answer>
  hwaiting-agent comment <card_id> <text>
  hwaiting-agent lookups

login reads HWAITING_USERNAME and HWAITING_PASSWORD from the environment.
HWAITING_API_URL overrides the API base URL (default http://localhost:15000).
`)
}

func printJSON(raw []byte) {
	var indented bytes.Buffer
	if err := json.Indent(&indented, raw, "", "  "); err != nil {
		// Not JSON for some reason - print as-is rather than swallow it.
		os.Stdout.Write(raw)
		fmt.Println()
		return
	}
	indented.WriteByte('\n')
	os.Stdout.Write(indented.Bytes())
}

func cmdLogin(args []string) ([]byte, error) {
	if len(args) != 0 {
		return nil, fmt.Errorf("login takes no arguments")
	}
	username := os.Getenv("HWAITING_USERNAME")
	password := os.Getenv("HWAITING_PASSWORD")
	if username == "" || password == "" {
		return nil, fmt.Errorf("HWAITING_USERNAME and HWAITING_PASSWORD must be set in the environment")
	}
	return login(username, password)
}

func cmdReview(args []string) ([]byte, error) {
	if len(args) != 0 {
		return nil, fmt.Errorf("review takes no arguments")
	}
	return review()
}

func cmdAnswer(args []string) ([]byte, error) {
	if len(args) != 2 {
		return nil, fmt.Errorf("usage: hwaiting-agent answer <card_id> <answer>")
	}
	return answer(args[0], args[1])
}

func cmdComment(args []string) ([]byte, error) {
	if len(args) != 2 {
		return nil, fmt.Errorf("usage: hwaiting-agent comment <card_id> <text>")
	}
	return comment(args[0], args[1])
}

func cmdLookups(args []string) ([]byte, error) {
	if len(args) != 0 {
		return nil, fmt.Errorf("lookups takes no arguments")
	}
	return lookups()
}
