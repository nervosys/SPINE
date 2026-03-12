package spine

import (
	"testing"
)

func TestVersion(t *testing.T) {
	v := Version()
	if v == "" {
		t.Fatal("Version() returned empty string")
	}
	if v != "1.0.0" {
		t.Fatalf("expected version 1.0.0, got %s", v)
	}
}

func TestParseHTML(t *testing.T) {
	html := `<html><head><title>Test Page</title></head><body><p>Hello World</p></body></html>`
	ur, err := ParseHTML(html)
	if err != nil {
		t.Fatalf("ParseHTML failed: %v", err)
	}
	if ur.Title != "Test Page" {
		t.Fatalf("expected title 'Test Page', got '%s'", ur.Title)
	}
}

func TestCompileHLS(t *testing.T) {
	source := "let x = 42"
	binary, err := CompileHLS(source)
	if err != nil {
		t.Fatalf("CompileHLS failed: %v", err)
	}
	if len(binary.Instructions) == 0 {
		t.Fatal("expected non-empty instructions")
	}
}

func TestCompileHLSInvalid(t *testing.T) {
	_, err := CompileHLS("@@@invalid@@@")
	if err == nil {
		t.Fatal("expected error for invalid HLS source")
	}
}

func TestConnectInvalidAddr(t *testing.T) {
	_, err := Connect("invalid:99999")
	if err == nil {
		t.Fatal("expected error for invalid address")
	}
}

func TestClientCloseNil(t *testing.T) {
	c := &Client{}
	c.Close() // should not panic
}
